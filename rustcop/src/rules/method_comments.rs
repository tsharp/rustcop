// rustcop::ignore-file: Tests include literal suppression directives for method exceptions.

use std::path::Path;

use serde::Deserialize;
use syn::{spanned::Spanned, visit::Visit};

use crate::{
    config::{Config, LintConfig},
    diagnostic::{Diagnostic, Severity},
    rules::{documentation::has_documentation, Rule},
};

#[derive(Default, Deserialize)]
#[serde(default)]
struct MethodCommentsConfig {
    require_trait_method_docs: Option<bool>,
    // Compatibility with the earlier inheritance setting.
    inherit_trait_docs: Option<bool>,
}

/// Require documentation on all explicitly written impl and trait methods.
pub struct RequireMethodCommentsRule {
    severity: Option<Severity>,
    require_trait_method_docs: bool,
}

impl RequireMethodCommentsRule {
    pub fn from_config(config: &Config) -> Self {
        let lint = config
            .get_nested_config::<LintConfig>(&["lints", "require_method_comments"])
            .unwrap_or_default();
        let options = config
            .get_nested_config::<MethodCommentsConfig>(&["lints", "require_method_comments"])
            .unwrap_or_default();
        Self {
            severity: match lint.severity.as_str() {
                "error" => Some(Severity::Error),
                "warning" | "suggestion" => Some(Severity::Warning),
                _ => None,
            },
            require_trait_method_docs: options
                .require_trait_method_docs
                .unwrap_or_else(|| !options.inherit_trait_docs.unwrap_or(true)),
        }
    }
}

impl Rule for RequireMethodCommentsRule {
    fn id(&self) -> &str {
        "RC3001"
    }

    fn name(&self) -> &str {
        "RequireMethodComments"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        let Some(severity) = &self.severity else {
            return vec![];
        };
        let Ok(ast) = syn::parse_file(content) else {
            return vec![];
        };
        let mut visitor = MethodCommentsVisitor {
            file,
            severity: severity.clone(),
            diagnostics: vec![],
            require_trait_method_docs: self.require_trait_method_docs,
            in_trait_impl: false,
        };
        visitor.visit_file(&ast);
        visitor.diagnostics
    }

    fn fix(&self, content: &str) -> String {
        // Documentation must be written by the author.
        content.to_string()
    }
}

struct MethodCommentsVisitor<'a> {
    file: &'a Path,
    severity: Severity,
    diagnostics: Vec<Diagnostic>,
    require_trait_method_docs: bool,
    in_trait_impl: bool,
}

impl MethodCommentsVisitor<'_> {
    fn check_method(&mut self, attrs: &[syn::Attribute], signature: &syn::Signature) {
        let documented = (self.in_trait_impl && !self.require_trait_method_docs)
            || has_documentation(attrs, false);
        if !documented {
            self.diagnostics.push(Diagnostic {
                rule_id: "RC3001".to_string(),
                message: format!(
                    "Method `{}` requires a documentation comment",
                    signature.ident
                ),
                file: self.file.to_path_buf(),
                line: signature.span().start().line,
                severity: self.severity.clone(),
                suppressed: false,
                suppression_justification: None,
            });
        }
    }
}

impl<'ast> Visit<'ast> for MethodCommentsVisitor<'_> {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.in_trait_impl;
        self.in_trait_impl = item.trait_.is_some();
        syn::visit::visit_item_impl(self, item);
        self.in_trait_impl = previous;
    }

    fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
        self.check_method(&method.attrs, &method.sig);
        syn::visit::visit_impl_item_fn(self, method);
    }

    fn visit_trait_item_fn(&mut self, method: &'ast syn::TraitItemFn) {
        let previous = self.in_trait_impl;
        self.in_trait_impl = false;
        self.check_method(&method.attrs, &method.sig);
        syn::visit::visit_trait_item_fn(self, method);
        self.in_trait_impl = previous;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suppression::SuppressionParser;

    fn rule() -> RequireMethodCommentsRule {
        RequireMethodCommentsRule {
            severity: Some(Severity::Error),
            require_trait_method_docs: false,
        }
    }

    #[test]
    fn checks_private_public_associated_and_trait_methods() {
        let content = r#"struct S;
impl S {
    pub fn new() -> Self { S }
    fn private(&self) {}
    pub(crate) async fn restricted(&self) {}
}
trait T {
    fn declaration(&self);
    fn default_method(&self) {}
}
impl T for S {
    fn declaration(&self) {}
}
fn free_function() {}
"#;
        let diagnostics = rule().check(content, Path::new("test.rs"));
        assert_eq!(
            diagnostics.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![3, 4, 5, 8, 9]
        );
        assert_eq!(rule().fix(content), content);
    }

    #[test]
    fn accepts_doc_forms_but_rejects_empty_docs_hidden_and_ordinary_comments() {
        let content = r#"impl S {
    /// Construct a value.
    fn line_doc() {}
    /** Perform the operation. */
    fn block_doc() {}
    #[doc = "Generated description"]
    fn attr_doc() {}
    #[doc = include_str!("method.md")]
    fn included_doc() {}
    // Ordinary comment.
    fn ordinary_line() {}
    /* Ordinary block. */
    fn ordinary_block() {}
    ///
    fn empty_doc() {}
    #[doc(hidden)]
    fn hidden() {}
}
"#;
        let diagnostics = rule().check(content, Path::new("test.rs"));
        assert_eq!(
            diagnostics.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![11, 13, 15, 17]
        );
    }

    #[test]
    fn honors_configured_severity_and_defaults_to_disabled() {
        let content = "impl S { fn missing() {} }";
        assert!(RequireMethodCommentsRule::from_config(&Config::empty())
            .check(content, Path::new("test.rs"))
            .is_empty());
        for (severity, count) in [("none", 0), ("warning", 1), ("error", 1)] {
            let mut config = Config::empty();
            config.raw = toml::from_str(&format!(
                "[lints.require_method_comments]\nseverity = \"{severity}\""
            ))
            .unwrap();
            let diagnostics = RequireMethodCommentsRule::from_config(&config)
                .check(content, Path::new("test.rs"));
            assert_eq!(diagnostics.len(), count);
            if count > 0 {
                assert_eq!(
                    matches!(diagnostics[0].severity, Severity::Error),
                    severity == "error"
                );
            }
        }
    }

    #[test]
    fn method_exceptions_are_tracked() {
        let content = r#"impl S {
    // rustcop::ignore RC3001: Legacy method
    fn legacy() {}
    #[rustcop::ignore(RC3001, justification = "External trait API")]
    fn external() {}
    fn missing() {}
}
"#;
        let mut parser = SuppressionParser::parse(content);
        let suppressed: Vec<_> = rule()
            .check(content, Path::new("test.rs"))
            .iter()
            .map(|d| parser.is_suppressed(d.line, &d.rule_id).0)
            .collect();
        assert_eq!(suppressed, vec![true, true, false]);
        assert!(parser.get_unused_suppressions().is_empty());
        assert!(parser.get_suppressions_without_justification().is_empty());
    }

    #[test]
    fn detects_trait_impls_without_markers_and_preserves_nested_context() {
        let content = r#"trait T {
    /// <inheritdoc />
    fn declaration(&self);
}
impl T for S {
    /// <inheritdoc/>
    fn declaration(&self) {}
    fn missing(&self) {}
}
/// <inheritdoc />
impl T for Other {
    fn declaration(&self) {
        impl Local { fn local() {} }
    }
}
/// <inheritdoc />
impl S {
    /// <inheritdoc />
    fn inherent(&self) {}
}
"#;
        let diagnostics = rule().check(content, Path::new("test.rs"));
        assert_eq!(
            diagnostics.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![3, 13, 19]
        );
    }

    #[test]
    fn inheritance_can_be_disabled_and_real_docs_still_count() {
        let content = r#"/// <inheritdoc />
impl T for S {
    /// <inheritdoc />
    fn inherited(&self) {}
    /// Own method documentation.
    fn documented(&self) {}
    fn missing(&self) {}
}"#;
        let mut config = Config::empty();
        config.raw = toml::from_str(
            "[lints.require_method_comments]\nseverity = \"error\"\ninherit_trait_docs = false",
        )
        .unwrap();
        let diagnostics =
            RequireMethodCommentsRule::from_config(&config).check(content, Path::new("test.rs"));
        assert_eq!(
            diagnostics.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![4, 7]
        );
    }

    #[test]
    fn html_comment_marker_inherits_without_becoming_ordinary_documentation() {
        let content = "/// <!-- <inheritdoc /> -->\nimpl T for S { fn inherited(&self) {} }\nimpl S {\n/// <!-- <inheritdoc/> -->\nfn missing(&self) {}\n}";
        let diagnostics = rule().check(content, Path::new("test.rs"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, 5);
    }

    #[test]
    fn trait_methods_are_automatically_optional_or_explicitly_required() {
        let content = "impl external::Trait for S {\nfn inherited(&self) {}\n/// Implementation-specific method docs.\nfn documented(&self) {}\n}";
        assert!(rule().check(content, Path::new("test.rs")).is_empty());
        let mut config = Config::empty();
        config.raw = toml::from_str("[lints.require_method_comments]\nseverity = \"error\"\nrequire_trait_method_docs = true").unwrap();
        let diagnostics =
            RequireMethodCommentsRule::from_config(&config).check(content, Path::new("test.rs"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, 2);
    }
}
