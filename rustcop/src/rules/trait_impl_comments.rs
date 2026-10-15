// rustcop::ignore-file: Tests include suppression directives as source fixtures.

use std::path::Path;

use syn::{spanned::Spanned, visit::Visit};

use crate::{
    config::{Config, LintConfig},
    diagnostic::{Diagnostic, Severity},
    rules::{documentation::has_documentation, Rule},
};

/// Require documentation describing each explicitly written trait implementation.
pub struct RequireTraitImplCommentsRule {
    severity: Option<Severity>,
}

impl RequireTraitImplCommentsRule {
    pub fn from_config(config: &Config) -> Self {
        // Impl documentation is required by default when method documentation
        // checking is enabled. An explicit setting can disable or change it.
        let lint = if config
            .raw()
            .get("lints")
            .and_then(|lints| lints.get("require_trait_impl_comments"))
            .is_some()
        {
            config
                .get_nested_config::<LintConfig>(&["lints", "require_trait_impl_comments"])
                .unwrap_or_default()
        } else {
            config
                .get_nested_config::<LintConfig>(&["lints", "require_method_comments"])
                .unwrap_or_default()
        };
        Self {
            severity: match lint.severity.as_str() {
                "error" => Some(Severity::Error),
                "warning" | "suggestion" => Some(Severity::Warning),
                _ => None,
            },
        }
    }
}

impl Rule for RequireTraitImplCommentsRule {
    fn id(&self) -> &str {
        "RC3003"
    }
    fn name(&self) -> &str {
        "RequireTraitImplComments"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        let Some(severity) = &self.severity else {
            return vec![];
        };
        let Ok(ast) = syn::parse_file(content) else {
            return vec![];
        };
        let mut visitor = TraitImplVisitor {
            file,
            severity: severity.clone(),
            diagnostics: vec![],
        };
        visitor.visit_file(&ast);
        visitor.diagnostics
    }

    fn fix(&self, content: &str) -> String {
        content.to_string()
    }
}

struct TraitImplVisitor<'a> {
    file: &'a Path,
    severity: Severity,
    diagnostics: Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for TraitImplVisitor<'_> {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if item.trait_.is_some() && !has_documentation(&item.attrs, false) {
            self.diagnostics.push(Diagnostic {
                rule_id: "RC3003".to_string(),
                message: "Trait implementation requires its own documentation comment".to_string(),
                file: self.file.to_path_buf(),
                line: item.impl_token.span().start().line,
                severity: self.severity.clone(),
                suppressed: false,
                suppression_justification: None,
            });
        }
        syn::visit::visit_item_impl(self, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suppression::SuppressionParser;

    fn configured(text: &str) -> RequireTraitImplCommentsRule {
        let mut config = Config::empty();
        config.raw = toml::from_str(text).unwrap();
        RequireTraitImplCommentsRule::from_config(&config)
    }

    #[test]
    fn required_with_method_checking_and_independently_configurable() {
        let content = "impl T for S {}";
        for (config, count) in [
            ("", 0),
            ("[lints.require_method_comments]\nseverity = \"error\"", 1),
            ("[lints.require_trait_impl_comments]\nseverity = \"warning\"", 1),
            ("[lints.require_method_comments]\nseverity = \"error\"\n[lints.require_trait_impl_comments]\nseverity = \"none\"", 0),
        ] {
            assert_eq!(configured(config).check(content, Path::new("test.rs")).len(), count);
        }
    }

    #[test]
    fn checks_impl_blocks_even_when_their_methods_have_docs() {
        let content = "impl S {}\nimpl T for S {\n/// Method docs.\nfn f() {}\n}\n/// Implementation docs.\nimpl T for Other {}\n/// <!-- <inheritdoc /> -->\nimpl T for Third {}";
        let rule = configured("[lints.require_trait_impl_comments]\nseverity = \"error\"");
        let diagnostics = rule.check(content, Path::new("test.rs"));
        assert_eq!(
            diagnostics.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![2, 9]
        );
        assert_eq!(rule.fix(content), content);
    }

    #[test]
    fn impl_exception_is_independent_of_method_exceptions() {
        let content = "// rustcop::ignore RC3003: Standard behavior\nimpl T for S {}";
        let mut parser = SuppressionParser::parse(content);
        let diagnostics = configured("[lints.require_trait_impl_comments]\nseverity = \"error\"")
            .check(content, Path::new("test.rs"));
        assert!(
            parser
                .is_suppressed(diagnostics[0].line, &diagnostics[0].rule_id)
                .0
        );
        assert!(!parser.is_suppressed(2, "RC3001").0);
        assert!(parser.get_unused_suppressions().is_empty());
    }
}
