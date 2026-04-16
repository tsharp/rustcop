/// Represents a suppression directive
#[derive(Debug, Clone, PartialEq)]
pub enum Suppression {
    /// Suppress all rules for the entire file
    FileLevel {
        directive_line: usize,
        justification: Option<String>,
    },
    /// Suppress all rules on a specific line
    LineLevel {
        directive_line: usize,
        line: usize,
        justification: Option<String>,
    },
    /// Suppress a specific rule on a specific line
    SpecificRule {
        directive_line: usize,
        line: usize,
        rule: String,
        justification: Option<String>,
    },
}

/// Parse suppression directives from source code
pub struct SuppressionParser {
    suppressions: Vec<Suppression>,
    used: std::collections::HashSet<usize>,
}

impl SuppressionParser {
    /// Parse suppressions from source code
    pub fn parse(content: &str) -> Self {
        let mut suppressions = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        // Check for file-level suppression
        if let Some(directive_line) = Self::find_file_level_suppression(content) {
            suppressions.push(Suppression::FileLevel {
                directive_line,
                justification: None,
            });
            return Self {
                suppressions,
                used: std::collections::HashSet::new(),
            };
        }

        // Parse comment-based suppressions line-by-line
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1; // 1-based line numbers
            let line_suppressions = Self::parse_comment_suppressions(line, line_num);
            suppressions.extend(line_suppressions);
        }

        // Parse attribute-based suppressions using syn
        suppressions.extend(Self::parse_attribute_suppressions_syn(content));

        Self {
            suppressions,
            used: std::collections::HashSet::new(),
        }
    }

    /// Check if file has file-level suppression and return its line number
    fn find_file_level_suppression(content: &str) -> Option<usize> {
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate().take(20) {
            let trimmed = line.trim();

            // Check for comment-based file suppression
            if trimmed.starts_with("// rustcop:ignore-file")
                || trimmed.starts_with("//rustcop:ignore-file")
            {
                return Some(i + 1); // 1-based line number
            }

            // Check for attribute-based file suppression
            if trimmed.contains("#![rustcop::ignore]")
                || trimmed.contains("#![ rustcop :: ignore ]")
                || trimmed.contains("#![rustcop::allow]")
            {
                return Some(i + 1);
            }
        }

        None
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
        let directive_line = line_num; // This is where the directive itself appears

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
                        directive_line,
                        line: target_line,
                        rule,
                        justification: justification.clone(),
                    })
                    .collect();
            } else {
                // No specific rules, suppress all rules on the line
                return vec![Suppression::LineLevel {
                    directive_line,
                    line: target_line,
                    justification,
                }];
            }
        }

        Vec::new()
    }

    /// Parse attribute-based suppressions using text patterns and syn for validation
    fn parse_attribute_suppressions_syn(content: &str) -> Vec<Suppression> {
        let mut suppressions = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;
            let trimmed = line.trim();

            // Look for #[rustcop::ignore...] patterns
            if !trimmed.starts_with("#[rustcop::ignore") {
                continue;
            }

            // Extract the content between #[ and ]
            let attr_start = trimmed.find("#[rustcop::ignore");
            if attr_start.is_none() {
                continue;
            }

            // Find the end of the attribute
            let rest = &trimmed[attr_start.unwrap()..];
            let attr_end = rest.find(']');
            if attr_end.is_none() {
                continue;
            }

            let attr_text = &rest[..=attr_end.unwrap()];
            
            // The directive is on this line
            let directive_line = line_num;
            
            // The suppression applies to the next line (the item itself)
            let target_line = line_num + 1;

            // Parse the attribute content
            let (rules, justification) = if attr_text.contains('(') {
                // #[rustcop::ignore(RC1001, justification = "reason")]
                let start_paren = attr_text.find('(');
                let end_paren = attr_text.rfind(')');
                
                if let (Some(start), Some(end)) = (start_paren, end_paren) {
                    let args = &attr_text[start + 1..end];
                    Self::parse_attribute_args(args)
                } else {
                    (None, None)
                }
            } else {
                // #[rustcop::ignore] - no arguments
                (None, None)
            };

            // Create suppressions for this attribute
            if let Some(rules) = rules {
                // Specific rules
                for rule in rules {
                    suppressions.push(Suppression::SpecificRule {
                        directive_line,
                        line: target_line,
                        rule,
                        justification: justification.clone(),
                    });
                }
            } else {
                // Suppress all rules
                suppressions.push(Suppression::LineLevel {
                    directive_line,
                    line: target_line,
                    justification,
                });
            }
        }

        suppressions
    }

    /// Parse attribute arguments like "RC1001, justification = \"reason\""
    fn parse_attribute_args(tokens: &str) -> (Option<Vec<String>>, Option<String>) {
        let mut rules = Vec::new();
        let mut justification = None;

        // Split by comma and process each part
        for part in tokens.split(',') {
            let part = part.trim();
            
            if part.starts_with("justification") {
                // Extract justification value: justification = "text"
                if let Some(eq_pos) = part.find('=') {
                    let value = part[eq_pos + 1..].trim();
                    // Remove quotes
                    let value = value.trim_matches(|c| c == '"' || c == '\'');
                    justification = Some(value.to_string());
                }
            } else if !part.is_empty() && part.chars().next().unwrap().is_ascii_uppercase() {
                // This looks like a rule code (starts with uppercase)
                rules.push(part.to_string());
            }
        }

        let rules_opt = if rules.is_empty() { None } else { Some(rules) };
        (rules_opt, justification)
    }

    /// Check if a diagnostic should be suppressed
    pub fn is_suppressed(&mut self, line: usize, rule_id: &str) -> (bool, Option<String>) {
        for (idx, suppression) in self.suppressions.iter().enumerate() {
            match suppression {
                Suppression::FileLevel { justification, .. } => {
                    self.used.insert(idx);
                    return (true, justification.clone());
                }
                Suppression::LineLevel {
                    line: sup_line,
                    justification,
                    ..
                } if *sup_line == line => {
                    self.used.insert(idx);
                    return (true, justification.clone());
                }
                Suppression::SpecificRule {
                    line: sup_line,
                    rule,
                    justification,
                    ..
                } if *sup_line == line && rule == rule_id => {
                    self.used.insert(idx);
                    return (true, justification.clone());
                }
                _ => {}
            }
        }
        (false, None)
    }

    /// Get unused suppressions (suppressions that were never matched against a diagnostic)
    pub fn get_unused_suppressions(&self) -> Vec<UnusedSuppression> {
        self.suppressions
            .iter()
            .enumerate()
            .filter(|(idx, _)| !self.used.contains(idx))
            .map(|(_, suppression)| match suppression {
                Suppression::FileLevel { directive_line, .. } => UnusedSuppression {
                    directive_line: *directive_line,
                    description: "all rules for entire file".to_string(),
                },
                Suppression::LineLevel {
                    directive_line,
                    line,
                    ..
                } => UnusedSuppression {
                    directive_line: *directive_line,
                    description: format!("all rules on line {}", line),
                },
                Suppression::SpecificRule {
                    directive_line,
                    rule,
                    line,
                    ..
                } => UnusedSuppression {
                    directive_line: *directive_line,
                    description: format!("rule {} on line {}", rule, line),
                },
            })
            .collect()
    }

    /// Get suppressions that are missing justifications
    pub fn get_suppressions_without_justification(&self) -> Vec<&Suppression> {
        self.suppressions
            .iter()
            .filter(|s| match s {
                Suppression::FileLevel { justification, .. } => justification.is_none(),
                Suppression::LineLevel { justification, .. } => justification.is_none(),
                Suppression::SpecificRule { justification, .. } => justification.is_none(),
            })
            .collect()
    }
}

/// Information about an unused suppression
#[derive(Debug, Clone)]
pub struct UnusedSuppression {
    pub directive_line: usize,
    pub description: String,
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

        let mut parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 1);
        assert!(matches!(
            parser.suppressions[0],
            Suppression::FileLevel { .. }
        ));
        assert!(parser.is_suppressed(1, "RC1001").0);
        assert!(parser.is_suppressed(100, "RC9999").0);
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

        let mut parser = SuppressionParser::parse(content);
        assert!(parser.is_suppressed(3, "RC1001").0);
        assert!(parser.is_suppressed(3, "RC9999").0);
        assert!(!parser.is_suppressed(4, "RC1001").0);
    }

    #[test]
    fn test_line_level_suppression_specific_rules() {
        let content = r#"
// rustcop:ignore RC1001
use std::fs;
// rustcop:ignore RC1001, RC1002
use std::collections::HashMap;
"#;

        let mut parser = SuppressionParser::parse(content);
        assert!(parser.is_suppressed(3, "RC1001").0);
        assert!(!parser.is_suppressed(3, "RC1002").0);

        assert!(parser.is_suppressed(5, "RC1001").0);
        assert!(parser.is_suppressed(5, "RC1002").0);
        assert!(!parser.is_suppressed(5, "RC9999").0);
    }

    #[test]
    fn test_no_space_variant() {
        let content = r#"
//rustcop:ignore RC1001
use std::fs;
"#;

        let mut parser = SuppressionParser::parse(content);
        assert!(parser.is_suppressed(3, "RC1001").0);
    }

    #[test]
    fn test_no_suppressions() {
        let content = r#"
use std::fs;
use std::collections::HashMap;
"#;

        let mut parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 0);
        assert!(!parser.is_suppressed(2, "RC1001").0);
    }

    #[test]
    fn test_suppression_with_justification() {
        let content = r#"
// rustcop:ignore RC1001: This is a legacy API
use std::fs;
"#;

        let mut parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 1);
        let (is_suppressed, justification) = parser.is_suppressed(3, "RC1001");
        assert!(is_suppressed);
        assert_eq!(justification.as_deref(), Some("This is a legacy API"));

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

        let mut parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 2);
        assert!(parser.is_suppressed(3, "RC1001").0);
        assert!(parser.is_suppressed(3, "RC1002").0);

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

        let mut parser = SuppressionParser::parse(content);
        assert_eq!(parser.suppressions.len(), 2);

        // First suppression targets line 3 (line 2 + 1), second targets line 4 (line 3 + 1)
        assert!(parser.is_suppressed(3, "RC1001").0);
        assert!(parser.is_suppressed(4, "RC1002").0);

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

    #[test]
    fn test_unused_suppression_detection() {
        let content = r#"
// rustcop:ignore RC1001
use std::fs;
// rustcop:ignore RC1002
use std::collections::HashMap;
"#;

        let mut parser = SuppressionParser::parse(content);

        // Check RC1001 on line 3 - this will mark it as used
        assert!(parser.is_suppressed(3, "RC1001").0);

        // Don't check RC1002 - leave it unused

        // Get unused suppressions
        let unused = parser.get_unused_suppressions();
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].directive_line, 4); // The comment is on line 4
        assert_eq!(unused[0].description, "rule RC1002 on line 5");
    }

    #[test]
    fn test_all_suppressions_used() {
        let content = r#"
// rustcop:ignore RC1001
use std::fs;
// rustcop:ignore RC1002
use std::collections::HashMap;
"#;

        let mut parser = SuppressionParser::parse(content);

        // Check both suppressions
        parser.is_suppressed(3, "RC1001");
        parser.is_suppressed(5, "RC1002");

        // No unused suppressions
        let unused = parser.get_unused_suppressions();
        assert_eq!(unused.len(), 0);
    }

    #[test]
    fn test_attribute_suppression_basic() {
        let content = r#"
#[rustcop::ignore]
fn my_function() {
    println!("test");
}
"#;

        let mut parser = SuppressionParser::parse(content);
        assert!(!parser.suppressions.is_empty());
        
        // Should suppress line 3 (the function line)
        assert!(parser.is_suppressed(3, "RC1001").0);
        assert!(parser.is_suppressed(3, "RC9999").0); // Any rule
    }

    #[test]
    fn test_attribute_suppression_specific_rule() {
        let content = r#"
#[rustcop::ignore(RC1001)]
fn my_function() {
    println!("test");
}
"#;

        let mut parser = SuppressionParser::parse(content);
        
        // Should suppress RC1001 on line 3
        assert!(parser.is_suppressed(3, "RC1001").0);
        
        // Should NOT suppress other rules
        assert!(!parser.is_suppressed(3, "RC2001").0);
    }

    #[test]
    fn test_attribute_suppression_with_justification() {
        let content = r#"
#[rustcop::ignore(RC1001, justification = "Legacy code")]
fn my_function() {
    println!("test");
}
"#;

        let mut parser = SuppressionParser::parse(content);
        
        let (is_suppressed, justification) = parser.is_suppressed(3, "RC1001");
        assert!(is_suppressed);
        assert_eq!(justification.as_deref(), Some("Legacy code"));
    }

    #[test]
    fn test_attribute_suppression_multiple_rules() {
        let content = r#"
#[rustcop::ignore(RC1001, RC1002, justification = "Temporary")]
fn my_function() {
    println!("test");
}
"#;

        let mut parser = SuppressionParser::parse(content);
        
        assert!(parser.is_suppressed(3, "RC1001").0);
        assert!(parser.is_suppressed(3, "RC1002").0);
        assert!(!parser.is_suppressed(3, "RC3001").0);
    }

    #[test]
    fn test_attribute_unused_detection() {
        let content = r#"
#[rustcop::ignore(RC1001)]
fn my_function() {
    println!("test");
}
"#;

        let parser = SuppressionParser::parse(content);
        
        // Don't check any diagnostics - leave the suppression unused
        
        let unused = parser.get_unused_suppressions();
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].directive_line, 2); // The attribute is on line 2
        assert!(unused[0].description.contains("RC1001"));
    }
}
