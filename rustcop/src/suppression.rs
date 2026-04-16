/// Represents a suppression directive
#[derive(Debug, Clone, PartialEq)]
pub enum Suppression {
    /// Suppress all rules for the entire file
    FileLevel { justification: Option<String> },
    /// Suppress all rules on a specific line
    LineLevel {
        line: usize,
        justification: Option<String>,
    },
    /// Suppress a specific rule on a specific line
    SpecificRule {
        line: usize,
        rule: String,
        justification: Option<String>,
    },
}

/// Parse suppression directives from source code
pub struct SuppressionParser {
    suppressions: Vec<Suppression>,
}

impl SuppressionParser {
    /// Parse suppressions from source code
    pub fn parse(content: &str) -> Self {
        let mut suppressions = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        // Check for file-level suppression
        if Self::has_file_level_suppression(content) {
            suppressions.push(Suppression::FileLevel {
                justification: None,
            });
            return Self { suppressions };
        }

        // Parse line-by-line for comment-based suppressions
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1; // 1-based line numbers

            let line_suppressions = Self::parse_comment_suppressions(line, line_num);
            suppressions.extend(line_suppressions);
        }

        Self { suppressions }
    }

    /// Check if file has file-level suppression
    fn has_file_level_suppression(content: &str) -> bool {
        let lines: Vec<&str> = content.lines().take(20).collect(); // Check first 20 lines

        for line in lines {
            let trimmed = line.trim();

            // Check for comment-based file suppression
            if trimmed.starts_with("// rustcop:ignore-file")
                || trimmed.starts_with("//rustcop:ignore-file")
            {
                return true;
            }

            // Check for attribute-based file suppression
            if trimmed.contains("#![rustcop::ignore]")
                || trimmed.contains("#![ rustcop :: ignore ]")
                || trimmed.contains("#![rustcop::allow]")
            {
                return true;
            }
        }

        false
    }

    /// Extract justification from a suppression comment
    /// Justifications come after a colon: "// rustcop:ignore: Justification here"
    fn extract_justification(after_marker: &str) -> (Option<Vec<String>>, Option<String>) {
        // Look for colon to separate rules from justification
        if let Some(colon_pos) = after_marker.find(':') {
            let rules_part = &after_marker[..colon_pos].trim();
            let justification = after_marker[colon_pos + 1..].trim().to_string();

            if rules_part.is_empty() {
                // No rules, just justification: "// rustcop:ignore: justification"
                (None, Some(justification))
            } else {
                // Rules + justification: "// rustcop:ignore RC1001: justification"
                let rules: Vec<String> = rules_part
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                (Some(rules), Some(justification))
            }
        } else {
            // No justification, just parse rules
            if after_marker.is_empty() {
                (None, None)
            } else {
                let rules: Vec<String> = after_marker
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                (Some(rules), None)
            }
        }
    }

    /// Parse a comment-based suppression on a single line
    /// Returns a vector of suppressions since one comment can suppress multiple rules
    fn parse_comment_suppressions(line: &str, line_num: usize) -> Vec<Suppression> {
        let trimmed = line.trim();
        let target_line = line_num + 1; // Apply suppression to the NEXT line

        // Try both formats: "// rustcop:ignore" and "//rustcop:ignore"
        let after_marker = trimmed.find("// rustcop:ignore").map(|pos| trimmed[pos + 17..].trim())
            .or_else(|| trimmed.find("//rustcop:ignore").map(|pos| trimmed[pos + 16..].trim()));

        if let Some(after_marker) = after_marker {
            let (rules, justification) = Self::extract_justification(after_marker);

            if let Some(rules) = rules {
                // Create one suppression per rule, all with the same justification
                return rules
                    .into_iter()
                    .map(|rule| Suppression::SpecificRule {
                        line: target_line,
                        rule,
                        justification: justification.clone(),
                    })
                    .collect();
            } else {
                // No specific rules, suppress all rules on the line
                return vec![Suppression::LineLevel {
                    line: target_line,
                    justification,
                }];
            }
        }

        Vec::new()
    }

    /// Check if a diagnostic should be suppressed
    pub fn is_suppressed(&self, line: usize, rule_id: &str) -> bool {
        for suppression in &self.suppressions {
            match suppression {
                Suppression::FileLevel { .. } => return true,
                Suppression::LineLevel { line: sup_line, .. } if *sup_line == line => return true,
                Suppression::SpecificRule {
                    line: sup_line,
                    rule,
                    ..
                } if *sup_line == line && rule == rule_id => return true,
                _ => {}
            }
        }
        false
    }

    /// Get suppressions that are missing justifications
    pub fn get_suppressions_without_justification(&self) -> Vec<&Suppression> {
        self.suppressions
            .iter()
            .filter(|s| match s {
                Suppression::FileLevel { justification } => justification.is_none(),
                Suppression::LineLevel { justification, .. } => justification.is_none(),
                Suppression::SpecificRule { justification, .. } => justification.is_none(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_level_suppression_comment() {
        let content = r#"// rustcop:ignore-file
use std::fs;
use std::collections::HashMap;
"#;

        let parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 1);
        assert!(matches!(
            parser.suppressions[0],
            Suppression::FileLevel { .. }
        ));
        assert!(parser.is_suppressed(1, "RC1001"));
        assert!(parser.is_suppressed(100, "RC9999"));
    }

    #[test]
    fn test_file_level_suppression_attribute() {
        let content = r#"#![rustcop::ignore]
use std::fs;
"#;

        let parser = SuppressionParser::parse(content);
        assert!(matches!(
            parser.suppressions[0],
            Suppression::FileLevel { .. }
        ));
    }

    #[test]
    fn test_line_level_suppression_all_rules() {
        let content = r#"
// rustcop:ignore
use std::fs;
use std::collections::HashMap;
"#;

        let parser = SuppressionParser::parse(content);
        assert!(parser.is_suppressed(3, "RC1001"));
        assert!(parser.is_suppressed(3, "RC9999"));
        assert!(!parser.is_suppressed(4, "RC1001"));
    }

    #[test]
    fn test_line_level_suppression_specific_rules() {
        let content = r#"
// rustcop:ignore RC1001
use std::fs;
// rustcop:ignore RC1001, RC1002
use std::collections::HashMap;
"#;

        let parser = SuppressionParser::parse(content);
        assert!(parser.is_suppressed(3, "RC1001"));
        assert!(!parser.is_suppressed(3, "RC1002"));

        assert!(parser.is_suppressed(5, "RC1001"));
        assert!(parser.is_suppressed(5, "RC1002"));
        assert!(!parser.is_suppressed(5, "RC9999"));
    }

    #[test]
    fn test_no_space_variant() {
        let content = r#"
//rustcop:ignore RC1001
use std::fs;
"#;

        let parser = SuppressionParser::parse(content);
        assert!(parser.is_suppressed(3, "RC1001"));
    }

    #[test]
    fn test_no_suppressions() {
        let content = r#"
use std::fs;
use std::collections::HashMap;
"#;

        let parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 0);
        assert!(!parser.is_suppressed(2, "RC1001"));
    }

    #[test]
    fn test_suppression_with_justification() {
        let content = r#"
// rustcop:ignore RC1001: This is a legacy API
use std::fs;
"#;

        let parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 1);
        assert!(parser.is_suppressed(3, "RC1001"));

        if let Suppression::SpecificRule { justification, .. } = &parser.suppressions[0] {
            assert_eq!(justification.as_deref(), Some("This is a legacy API"));
        } else {
            panic!("Expected SpecificRule");
        }
    }

    #[test]
    fn test_multiple_rules_share_justification() {
        let content = r#"
// rustcop:ignore RC1001, RC1002: Performance critical section
use std::fs;
"#;

        let parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 2);
        assert!(parser.is_suppressed(3, "RC1001"));
        assert!(parser.is_suppressed(3, "RC1002"));

        // Both rules should share the same justification
        for suppression in &parser.suppressions {
            if let Suppression::SpecificRule { justification, .. } = suppression {
                assert_eq!(
                    justification.as_deref(),
                    Some("Performance critical section")
                );
            } else {
                panic!("Expected SpecificRule");
            }
        }
    }

    #[test]
    fn test_stacked_suppressions_different_justifications() {
        let content = r#"
// rustcop:ignore RC1001: Reason one
// rustcop:ignore RC1002: Reason two  
use std::fs;
"#;

        let parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 2);

        // First suppression targets line 3 (line 2 + 1), second targets line 4 (line 3 + 1)
        assert!(parser.is_suppressed(3, "RC1001"));
        assert!(parser.is_suppressed(4, "RC1002"));

        // Check each has different justification
        let rc1001 = parser
            .suppressions
            .iter()
            .find(|s| matches!(s, Suppression::SpecificRule { rule, .. } if rule == "RC1001"))
            .unwrap();

        let rc1002 = parser
            .suppressions
            .iter()
            .find(|s| matches!(s, Suppression::SpecificRule { rule, .. } if rule == "RC1002"))
            .unwrap();

        if let Suppression::SpecificRule { justification, .. } = rc1001 {
            assert_eq!(justification.as_deref(), Some("Reason one"));
        }

        if let Suppression::SpecificRule { justification, .. } = rc1002 {
            assert_eq!(justification.as_deref(), Some("Reason two"));
        }
    }

    #[test]
    fn test_get_suppressions_without_justification() {
        let content = r#"
// rustcop:ignore RC1001: With justification
// rustcop:ignore RC1002
use std::fs;
"#;

        let parser = SuppressionParser::parse(content);
        let without_justification = parser.get_suppressions_without_justification();

        assert_eq!(without_justification.len(), 1);
        if let Suppression::SpecificRule {
            rule,
            justification,
            ..
        } = without_justification[0]
        {
            assert_eq!(rule, "RC1002");
            assert!(justification.is_none());
        }
    }
}
