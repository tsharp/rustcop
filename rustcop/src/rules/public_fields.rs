// rustcop::ignore-file: Suppression fixtures below intentionally contain directives as source text.

use std::path::Path;

use serde::Deserialize;
use syn::{spanned::Spanned, visit::Visit};

use crate::{
    config::{Config, LintConfig},
    diagnostic::{Diagnostic, Severity},
    rules::Rule,
};

/// Maximum permitted scope for explicitly visible struct fields.
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AllowedVisibility {
    #[default]
    Crate,
    Module,
    None,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PublicFieldsConfig {
    allowed_visibility: AllowedVisibility,
}

impl AllowedVisibility {
    fn permits(self, visibility: &syn::Visibility) -> bool {
        match visibility {
            syn::Visibility::Inherited => true,
            syn::Visibility::Public(_) => false,
            syn::Visibility::Restricted(restricted) => match self {
                Self::Crate => true,
                Self::Module => !restricted.path.is_ident("crate"),
                Self::None => false,
            },
        }
    }
}

/// Reject struct fields exceeding the configured visibility scope.
pub struct DisallowPublicFieldsRule {
    severity: Option<Severity>,
    allowed_visibility: AllowedVisibility,
}

impl DisallowPublicFieldsRule {
    pub fn from_config(config: &Config) -> Self {
        let lint = config
            .get_nested_config::<LintConfig>(&["lints", "disallow_public_fields"])
            .unwrap_or_default();
        let fields = config
            .get_nested_config::<PublicFieldsConfig>(&["lints", "disallow_public_fields"])
            .unwrap_or_default();
        Self {
            severity: match lint.severity.as_str() {
                "error" => Some(Severity::Error),
                "warning" | "suggestion" => Some(Severity::Warning),
                _ => None,
            },
            allowed_visibility: fields.allowed_visibility,
        }
    }
}

impl Rule for DisallowPublicFieldsRule {
    fn id(&self) -> &str {
        "RC2003"
    }

    fn name(&self) -> &str {
        "DisallowPublicFields"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        let Some(severity) = &self.severity else {
            return vec![];
        };
        let Ok(ast) = syn::parse_file(content) else {
            return vec![];
        };
        let mut visitor = PublicFieldsVisitor {
            file,
            severity: severity.clone(),
            allowed_visibility: self.allowed_visibility,
            diagnostics: vec![],
        };
        visitor.visit_file(&ast);
        visitor.diagnostics
    }

    fn fix(&self, content: &str) -> String {
        content.to_string()
    }
}

struct PublicFieldsVisitor<'a> {
    file: &'a Path,
    severity: Severity,
    allowed_visibility: AllowedVisibility,
    diagnostics: Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for PublicFieldsVisitor<'_> {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        for (index, field) in item.fields.iter().enumerate() {
            if !self.allowed_visibility.permits(&field.vis) {
                let name = field
                    .ident
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| index.to_string());
                self.diagnostics.push(Diagnostic {
                    rule_id: "RC2003".to_string(),
                    message: format!(
                        "Field `{name}` on struct `{}` exceeds the allowed visibility",
                        item.ident
                    ),
                    file: self.file.to_path_buf(),
                    line: field.vis.span().start().line,
                    severity: self.severity.clone(),
                    suppressed: false,
                    suppression_justification: None,
                });
            }
        }
        syn::visit::visit_item_struct(self, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suppression::SuppressionParser;

    fn rule() -> DisallowPublicFieldsRule {
        DisallowPublicFieldsRule {
            severity: Some(Severity::Error),
            allowed_visibility: AllowedVisibility::Crate,
        }
    }

    #[test]
    fn checks_named_tuple_nested_and_local_structs_only() {
        let content = "struct Named { pub x: u8, y: u8, pub(crate) z: u8 }\n\
            mod nested { struct Tuple(pub u8, pub(super) u8); }\n\
            fn f() { struct Local { pub x: u8 } }\n\
            enum E { Variant { x: u8 } }\n\
            pub fn public_function() {}\n\
            // struct Fake { pub x: u8 }\n\
            const TEXT: &str = \"struct Fake { pub x: u8 }\";";
        let diagnostics = rule().check(content, Path::new("test.rs"));
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(
            diagnostics.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(rule().fix(content), content);
    }

    #[test]
    fn default_is_disabled() {
        assert!(DisallowPublicFieldsRule::from_config(&Config::empty())
            .check("struct S(pub u8);", Path::new("test.rs"))
            .is_empty());
    }

    #[test]
    fn configured_severity_is_respected() {
        for (value, expected) in [
            ("none", None),
            ("warning", Some(false)),
            ("error", Some(true)),
        ] {
            let mut config = Config::empty();
            config.raw = toml::from_str(&format!(
                "[lints.disallow_public_fields]\nseverity = \"{value}\""
            ))
            .unwrap();
            let diagnostics = DisallowPublicFieldsRule::from_config(&config)
                .check("struct S(pub u8);", Path::new("test.rs"));
            assert_eq!(diagnostics.len(), usize::from(expected.is_some()));
            if let Some(is_error) = expected {
                assert_eq!(matches!(diagnostics[0].severity, Severity::Error), is_error);
            }
        }
    }

    #[test]
    fn visibility_modes_cover_all_rust_visibility_forms() {
        let content = r#"mod child {
            struct S {
                private: u8,
                pub public: u8,
                pub(crate) crate_visible: u8,
                pub(in crate) crate_path: u8,
                pub(super) parent: u8,
                pub(self) local: u8,
                pub(in self) local_path: u8,
                pub(in crate::child) module_path: u8,
            }
        }"#;
        for (mode, expected) in [("crate", 1), ("module", 3), ("none", 7)] {
            let mut config = Config::empty();
            config.raw = toml::from_str(&format!(
                "[lints.disallow_public_fields]\nseverity = \"error\"\nallowed_visibility = \"{mode}\""
            )).unwrap();
            let diagnostics =
                DisallowPublicFieldsRule::from_config(&config).check(content, Path::new("test.rs"));
            assert_eq!(diagnostics.len(), expected, "mode {mode}");
        }
    }

    #[test]
    fn unused_struct_exception_and_missing_reason_are_reportable() {
        let content = "// rustcop::ignore RC2003\nstruct S { private: u8 }";
        let parser = SuppressionParser::parse(content);
        assert!(rule().check(content, Path::new("test.rs")).is_empty());
        assert_eq!(parser.get_unused_suppressions().len(), 1);
        assert_eq!(parser.get_suppressions_without_justification().len(), 1);
    }

    #[test]
    fn field_and_struct_exceptions_preserve_diagnostics_and_usage() {
        let content = r#"struct A {
    // rustcop::ignore RC2003: Required by wire format
    pub allowed: u8,
    pub rejected: u8,
}
#[rustcop::ignore(RC2003, justification = "Public data transfer object")]
#[derive(Clone)]
struct B {
    pub first: u8,
    pub second: u8,
}
// rustcop::ignore RC2003: Legacy tuple API
struct C(
    pub u8,
    pub u8,
);
struct D { pub rejected: u8 }
"#;
        let mut parser = SuppressionParser::parse(content);
        let diagnostics = rule().check(content, Path::new("test.rs"));
        let suppressed: Vec<_> = diagnostics
            .iter()
            .map(|d| parser.is_suppressed(d.line, &d.rule_id).0)
            .collect();
        assert_eq!(suppressed, vec![true, false, true, true, true, true, false]);
        assert!(parser.get_unused_suppressions().is_empty());
        assert!(parser.get_suppressions_without_justification().is_empty());
        assert!(!parser.is_suppressed(9, "RC2002").0);
    }
}
