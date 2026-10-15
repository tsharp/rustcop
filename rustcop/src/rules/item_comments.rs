// rustcop::ignore-file: Tests intentionally contain source-level suppression directives.

use std::{collections::HashSet, path::Path};

use serde::Deserialize;
use syn::{spanned::Spanned, visit::Visit};

use crate::{
    config::{Config, LintConfig},
    diagnostic::{Diagnostic, Severity},
    rules::{documentation::has_documentation, Rule},
};

/// Module-level declaration categories whose documentation can be required.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ItemKind {
    Structs,
    Enums,
    EnumVariants,
    Traits,
    Functions,
    Modules,
    Constants,
    Statics,
    TypeAliases,
}

#[derive(Deserialize)]
#[serde(default)]
struct ItemCommentsConfig {
    items: HashSet<ItemKind>,
}

impl Default for ItemCommentsConfig {
    fn default() -> Self {
        Self {
            items: [
                ItemKind::Structs,
                ItemKind::Enums,
                ItemKind::EnumVariants,
                ItemKind::Traits,
                ItemKind::Functions,
                ItemKind::Modules,
                ItemKind::Constants,
                ItemKind::Statics,
                ItemKind::TypeAliases,
            ]
            .into_iter()
            .collect(),
        }
    }
}

/// Require documentation independently on selected declarations and enum variants.
pub struct RequireItemCommentsRule {
    severity: Option<Severity>,
    items: HashSet<ItemKind>,
}

impl RequireItemCommentsRule {
    pub fn from_config(config: &Config) -> Self {
        let lint = config
            .get_nested_config::<LintConfig>(&["lints", "require_item_comments"])
            .unwrap_or_default();
        let options = config
            .get_nested_config::<ItemCommentsConfig>(&["lints", "require_item_comments"])
            .unwrap_or_default();
        Self {
            severity: match lint.severity.as_str() {
                "error" => Some(Severity::Error),
                "warning" | "suggestion" => Some(Severity::Warning),
                _ => None,
            },
            items: options.items,
        }
    }
}

impl Rule for RequireItemCommentsRule {
    fn id(&self) -> &str {
        "RC3002"
    }

    fn name(&self) -> &str {
        "RequireItemComments"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        let Some(severity) = &self.severity else {
            return vec![];
        };
        let Ok(ast) = syn::parse_file(content) else {
            return vec![];
        };
        let mut visitor = ItemCommentsVisitor {
            file,
            severity: severity.clone(),
            items: &self.items,
            diagnostics: vec![],
        };
        visitor.visit_file(&ast);
        visitor.diagnostics
    }

    fn fix(&self, content: &str) -> String {
        content.to_string()
    }
}

struct ItemCommentsVisitor<'a> {
    file: &'a Path,
    severity: Severity,
    items: &'a HashSet<ItemKind>,
    diagnostics: Vec<Diagnostic>,
}

impl ItemCommentsVisitor<'_> {
    fn check_item(
        &mut self,
        kind: ItemKind,
        attrs: &[syn::Attribute],
        name: &syn::Ident,
        line: usize,
    ) {
        if self.items.contains(&kind) && !has_documentation(attrs, false) {
            self.diagnostics.push(Diagnostic {
                rule_id: "RC3002".to_string(),
                message: format!(
                    "Declaration `{name}` ({kind:?}) requires a documentation comment"
                ),
                file: self.file.to_path_buf(),
                line,
                severity: self.severity.clone(),
                suppressed: false,
                suppression_justification: None,
            });
        }
    }
}

impl<'ast> Visit<'ast> for ItemCommentsVisitor<'_> {
    // Walk module declarations explicitly rather than expression bodies. Local
    // items and trait/impl associated items are outside this rule's scope.
    fn visit_item(&mut self, item: &'ast syn::Item) {
        match item {
            syn::Item::Struct(item) => self.check_item(
                ItemKind::Structs,
                &item.attrs,
                &item.ident,
                item.struct_token.span.start().line,
            ),
            syn::Item::Enum(item) => {
                self.check_item(
                    ItemKind::Enums,
                    &item.attrs,
                    &item.ident,
                    item.enum_token.span.start().line,
                );
                for variant in &item.variants {
                    self.check_item(
                        ItemKind::EnumVariants,
                        &variant.attrs,
                        &variant.ident,
                        variant.ident.span().start().line,
                    );
                }
            }
            syn::Item::Trait(item) => self.check_item(
                ItemKind::Traits,
                &item.attrs,
                &item.ident,
                item.trait_token.span.start().line,
            ),
            syn::Item::Fn(item) => self.check_item(
                ItemKind::Functions,
                &item.attrs,
                &item.sig.ident,
                item.sig.span().start().line,
            ),
            syn::Item::Mod(item) => {
                self.check_item(
                    ItemKind::Modules,
                    &item.attrs,
                    &item.ident,
                    item.mod_token.span.start().line,
                );
                if let Some((_, items)) = &item.content {
                    for child in items {
                        self.visit_item(child);
                    }
                }
            }
            syn::Item::Const(item) => self.check_item(
                ItemKind::Constants,
                &item.attrs,
                &item.ident,
                item.const_token.span.start().line,
            ),
            syn::Item::Static(item) => self.check_item(
                ItemKind::Statics,
                &item.attrs,
                &item.ident,
                item.static_token.span.start().line,
            ),
            syn::Item::Type(item) => self.check_item(
                ItemKind::TypeAliases,
                &item.attrs,
                &item.ident,
                item.type_token.span.start().line,
            ),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suppression::SuppressionParser;

    fn rule(items: &str) -> RequireItemCommentsRule {
        let mut config = Config::empty();
        config.raw = toml::from_str(&format!(
            "[lints.require_item_comments]\nseverity = \"error\"\n{items}"
        ))
        .unwrap();
        RequireItemCommentsRule::from_config(&config)
    }

    #[test]
    fn covers_each_category_and_inline_modules_but_not_function_locals() {
        let content = "struct S;\nenum E { A, B(u8), C { x: u8 } }\ntrait T {}\nfn f() { struct Local; }\nmod m { struct Nested; }\nconst C: u8 = 0;\nstatic V: u8 = 0;\ntype Alias = u8;";
        let diagnostics = rule("").check(content, Path::new("test.rs"));
        assert_eq!(diagnostics.len(), 12);
        assert!(!diagnostics.iter().any(|d| d.message.contains("Local")));
        assert_eq!(rule("").fix(content), content);
    }

    #[test]
    fn enum_and_variants_are_independent_and_selectable() {
        let content = "/// An enum.\nenum E {\n/// Documented variant.\nA,\nB,\n}\nenum F {\n/// Documented variant.\nC\n}";
        let diagnostics = rule("").check(content, Path::new("test.rs"));
        assert_eq!(
            diagnostics.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![5, 7]
        );
        assert_eq!(
            rule("items = [\"enum_variants\"]")
                .check(content, Path::new("test.rs"))
                .len(),
            1
        );
        assert_eq!(
            rule("items = []")
                .check(content, Path::new("test.rs"))
                .len(),
            0
        );
    }

    #[test]
    fn inner_module_docs_count_and_inheritance_has_no_parent() {
        let content = "mod m {\n//! Module docs.\n/// Struct docs.\nstruct S;\n}\n/// <inheritdoc />\nstruct Missing;";
        let diagnostics = rule("").check(content, Path::new("test.rs"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, 7);
    }

    #[test]
    fn exceptions_target_each_declaration_or_variant_individually() {
        let content = "// rustcop::ignore RC3002: Legacy enum\nenum E {\n// rustcop::ignore RC3002: Legacy variant\nA,\nB,\n}";
        let mut parser = SuppressionParser::parse(content);
        let suppressed: Vec<_> = rule("")
            .check(content, Path::new("test.rs"))
            .iter()
            .map(|d| parser.is_suppressed(d.line, &d.rule_id).0)
            .collect();
        assert_eq!(suppressed, vec![true, true, false]);
        assert!(parser.get_unused_suppressions().is_empty());
    }

    #[test]
    fn disabled_by_default_and_warning_is_respected() {
        let content = "struct S;";
        assert!(RequireItemCommentsRule::from_config(&Config::empty())
            .check(content, Path::new("test.rs"))
            .is_empty());
        let mut config = Config::empty();
        config.raw =
            toml::from_str("[lints.require_item_comments]\nseverity = \"warning\"").unwrap();
        assert!(matches!(
            RequireItemCommentsRule::from_config(&config).check(content, Path::new("test.rs"))[0]
                .severity,
            Severity::Warning
        ));
    }
}
