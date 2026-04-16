use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    config::{Config, LintConfig},
    diagnostic::{Diagnostic, Severity},
    rules::Rule,
};

/// Configuration for disallow_super_imports rule
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisallowSuperImportsConfig {
    /// Allow super imports in test modules
    pub allow_in_tests: bool,
}

/// Rule that disallows `super::` imports
pub struct DisallowSuperImportsRule {
    enabled: bool,
    allow_in_tests: bool,
}

impl DisallowSuperImportsRule {
    pub fn from_config(config: &Config) -> Self {
        let lint_config = config
            .get_nested_config::<LintConfig>(&["lints", "disallow_super_imports"])
            .unwrap_or_default();

        let enabled = lint_config.severity != "none";

        let rule_config = config
            .get_nested_config::<DisallowSuperImportsConfig>(&["lints", "disallow_super_imports"])
            .unwrap_or_default();

        Self {
            enabled,
            allow_in_tests: rule_config.allow_in_tests,
        }
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

        // If allow_in_tests is enabled and this is a test file, skip all checks
        if self.allow_in_tests && is_test_file(content) {
            return vec![];
        }

        let mut diagnostics = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let test_regions = if self.allow_in_tests {
            find_test_regions(&lines)
        } else {
            vec![]
        };

        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = line.trim();

            // Skip if in test region
            if self.allow_in_tests
                && test_regions
                    .iter()
                    .any(|(start, end)| line_idx >= *start && line_idx <= *end)
            {
                continue;
            }

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

/// Detect if entire file is marked as test (e.g., integration tests in tests/)
fn is_test_file(content: &str) -> bool {
    // Check for #![cfg(test)] at file level
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "#![cfg(test)]"
    })
}

/// Find regions that are test modules
/// Returns Vec of (start_line, end_line) inclusive, 0-indexed
fn find_test_regions(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Look for #[cfg(test)] followed by mod
        if trimmed == "#[cfg(test)]" && i + 1 < lines.len() {
            let next_trimmed = lines[i + 1].trim();
            if next_trimmed.starts_with("mod ") {
                // Find the closing brace
                if let Some(end) = find_closing_brace(lines, i + 1) {
                    regions.push((i, end));
                    i = end + 1;
                    continue;
                }
            }
        }

        // Also look for inline #[cfg(test)] mod
        if trimmed.starts_with("#[cfg(test)] mod ") {
            if let Some(end) = find_closing_brace(lines, i) {
                regions.push((i, end));
                i = end + 1;
                continue;
            }
        }

        i += 1;
    }

    regions
}

/// Find closing brace for a module starting at start_line
fn find_closing_brace(lines: &[&str], start_line: usize) -> Option<usize> {
    let mut depth = 0;
    let mut found_opening = false;

    for (offset, line) in lines[start_line..].iter().enumerate() {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    found_opening = true;
                }
                '}' => {
                    depth -= 1;
                    if found_opening && depth == 0 {
                        return Some(start_line + offset);
                    }
                }
                _ => {}
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_super_import() {
        let content = "use super::foo;\n";
        let rule = DisallowSuperImportsRule {
            enabled: true,
            allow_in_tests: false,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "RC2001");
    }

    #[test]
    fn test_detects_pub_super_import() {
        let content = "pub use super::bar;\n";
        let rule = DisallowSuperImportsRule {
            enabled: true,
            allow_in_tests: false,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_allows_other_imports() {
        let content = "use std::fs;\nuse crate::foo;\n";
        let rule = DisallowSuperImportsRule {
            enabled: true,
            allow_in_tests: false,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_disabled_when_severity_none() {
        let content = "use super::foo;\n";
        let rule = DisallowSuperImportsRule {
            enabled: false,
            allow_in_tests: false,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_allows_super_in_test_module() {
        let content = r#"
use std::fs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // test code
    }
}
"#;
        let rule = DisallowSuperImportsRule {
            enabled: true,
            allow_in_tests: true,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_disallows_super_outside_test_module() {
        let content = r#"
use super::foo;

#[cfg(test)]
mod tests {
    use super::*;
}
"#;
        let rule = DisallowSuperImportsRule {
            enabled: true,
            allow_in_tests: true,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        // Should flag the first super import, not the one in tests
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 2);
    }

    #[test]
    fn test_allows_super_when_allow_in_tests_disabled() {
        let content = r#"
#[cfg(test)]
mod tests {
    use super::*;
}
"#;
        let rule = DisallowSuperImportsRule {
            enabled: true,
            allow_in_tests: false,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        // Should still flag it when allow_in_tests is false
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_allows_super_in_test_file() {
        let content = r#"
#![cfg(test)]

use super::*;

#[test]
fn test_something() {
    // test code
}
"#;
        let rule = DisallowSuperImportsRule {
            enabled: true,
            allow_in_tests: true,
        };
        let diags = rule.check(content, Path::new("test.rs"));
        assert_eq!(diags.len(), 0);
    }
}
