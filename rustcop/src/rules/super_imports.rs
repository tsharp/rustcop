use std::path::Path;

use crate::{
    config::{Config, LintConfig},
    diagnostic::{Diagnostic, Severity},
    rules::Rule,
};

/// Rule that disallows `super::` imports
pub struct DisallowSuperImportsRule {
    enabled: bool,
}

impl DisallowSuperImportsRule {
    pub fn from_config(config: &Config) -> Self {
        let lint_config = config
            .get_nested_config::<LintConfig>(&["lints", "disallow_super_imports"])
            .unwrap_or_default();

        let enabled = lint_config.severity != "none";

        Self { enabled }
    }
}

impl Rule for DisallowSuperImportsRule {
    fn id(&self) -> &str {
        "RC2001"
    }

    fn name(&self) -> &str {
        "DisallowSuperImports"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        if !self.enabled {
            return vec![];
        }

        let mut diagnostics = Vec::new();

        for (line_idx, line) in content.lines().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = line.trim();

            // Check for `use super::` patterns
            if trimmed.starts_with("use super::") || trimmed.starts_with("pub use super::") {
                diagnostics.push(Diagnostic {
                    rule_id: self.id().to_string(),
                    message: "Use of `super::` imports is disallowed".to_string(),
                    file: file.to_path_buf(),
                    line: line_num,
                    severity: Severity::Error,
                    suppressed: false,
                    suppression_justification: None,
                });
            }
        }

        diagnostics
    }

    fn fix(&self, content: &str) -> String {
        // No auto-fix for this rule
        content.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_super_import() {
        let content = "use super::foo;\n";
        let rule = DisallowSuperImportsRule { enabled: true };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "RC2001");
    }

    #[test]
    fn test_detects_pub_super_import() {
        let content = "pub use super::bar;\n";
        let rule = DisallowSuperImportsRule { enabled: true };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_allows_other_imports() {
        let content = "use std::fs;\nuse crate::foo;\n";
        let rule = DisallowSuperImportsRule { enabled: true };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_disabled_when_severity_none() {
        let content = "use super::foo;\n";
        let rule = DisallowSuperImportsRule { enabled: false };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 0);
    }
}
