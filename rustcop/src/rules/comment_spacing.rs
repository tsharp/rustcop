use std::path::Path;

use crate::{
    config::{CommentSpacingConfig, Config},
    diagnostic::{Diagnostic, Severity},
    rules::{configured_severity, Rule},
};

pub struct CommentSpacingRule {
    severity: Option<Severity>,
}

impl CommentSpacingRule {
    pub fn from_config(config: &Config) -> Self {
        let spacing = config
            .get_config::<CommentSpacingConfig>("comment_spacing")
            .unwrap_or_default();
        Self {
            severity: configured_severity(&spacing.severity),
        }
    }
}

impl Rule for CommentSpacingRule {
    fn id(&self) -> &str {
        "RC1002"
    }

    fn name(&self) -> &str {
        "CommentBlockSpacing"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        let Some(severity) = &self.severity else {
            return Vec::new();
        };
        let Some(line) = first_spacing_violation(content) else {
            return Vec::new();
        };

        vec![Diagnostic {
            rule_id: self.id().to_string(),
            message: "Different comment block types must be separated by one blank line. Run `rustcop fix` to auto-fix."
                .to_string(),
            file: file.to_path_buf(),
            line,
            severity: severity.clone(),
            suppressed: false,
            suppression_justification: None,
        }]
    }

    fn fix(&self, content: &str) -> String {
        if self.severity.is_some() {
            normalize_comment_spacing(content)
        } else {
            content.to_string()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommentKind {
    Line,
    OuterDocLine,
    InnerDocLine,
    Block,
    OuterDocBlock,
    InnerDocBlock,
}

fn first_spacing_violation(content: &str) -> Option<usize> {
    let lines: Vec<&str> = content.lines().collect();
    let mut index = 0;

    while index < lines.len() {
        let Some((kind, end)) = comment_block_at(&lines, index) else {
            index += 1;
            continue;
        };

        let next = next_nonblank_line(&lines, end + 1);
        if let Some((next_kind, _)) = comment_block_at(&lines, next) {
            let blank_lines = next.saturating_sub(end + 1);
            if kind != next_kind && blank_lines != 1 {
                return Some(next + 1);
            }
        }

        index = end + 1;
    }

    None
}

fn normalize_comment_spacing(content: &str) -> String {
    if first_spacing_violation(content).is_none() {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let has_trailing_newline = content.ends_with('\n');
    let mut result = Vec::with_capacity(lines.len());
    let mut cursor = 0;
    let mut index = 0;

    while index < lines.len() {
        let Some((kind, end)) = comment_block_at(&lines, index) else {
            index += 1;
            continue;
        };

        let next = next_nonblank_line(&lines, end + 1);
        if let Some((next_kind, _)) = comment_block_at(&lines, next) {
            if kind != next_kind && next.saturating_sub(end + 1) != 1 {
                result.extend_from_slice(&lines[cursor..=end]);
                result.push("");
                cursor = next;
            }
        }

        index = end + 1;
    }

    result.extend_from_slice(&lines[cursor..]);
    let mut normalized = result.join(newline);
    if has_trailing_newline {
        normalized.push_str(newline);
    }
    normalized
}

fn next_nonblank_line(lines: &[&str], mut index: usize) -> usize {
    while index < lines.len() && lines[index].trim().is_empty() {
        index += 1;
    }
    index
}

fn comment_block_at(lines: &[&str], start: usize) -> Option<(CommentKind, usize)> {
    let trimmed = lines.get(start)?.trim_start();
    let (kind, block_comment) = classify_comment(trimmed)?;

    if block_comment {
        let mut end = start;
        while end < lines.len() {
            if let Some(close) = lines[end].find("*/") {
                if !lines[end][close + 2..].trim().is_empty() {
                    return None;
                }
                return Some((kind, end));
            }
            end += 1;
        }
        return None;
    }

    let mut end = start;
    while end + 1 < lines.len()
        && classify_comment(lines[end + 1].trim_start()) == Some((kind, false))
    {
        end += 1;
    }
    Some((kind, end))
}

fn classify_comment(line: &str) -> Option<(CommentKind, bool)> {
    if line.starts_with("//!") {
        Some((CommentKind::InnerDocLine, false))
    } else if line.starts_with("///") && !line.starts_with("////") {
        Some((CommentKind::OuterDocLine, false))
    } else if line.starts_with("//") {
        Some((CommentKind::Line, false))
    } else if line.starts_with("/*!") {
        Some((CommentKind::InnerDocBlock, true))
    } else if line.starts_with("/**") && !line.starts_with("/***") {
        Some((CommentKind::OuterDocBlock, true))
    } else if line.starts_with("/*") {
        Some((CommentKind::Block, true))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_blank_line_between_block_and_inner_doc_comments() {
        let input = concat!(
            "/*\n",
            " * Copyright notice\n",
            " */\n",
            "//! Module documentation.\n",
        );
        let expected = concat!(
            "/*\n",
            " * Copyright notice\n",
            " */\n",
            "\n",
            "//! Module documentation.\n",
        );

        assert_eq!(normalize_comment_spacing(input), expected);
    }

    #[test]
    fn collapses_multiple_blank_lines_between_different_comment_types() {
        let input = "/* header */\n\n\n/// Item documentation.\nstruct Item;\n";
        let expected = "/* header */\n\n/// Item documentation.\nstruct Item;\n";

        assert_eq!(normalize_comment_spacing(input), expected);
    }

    #[test]
    fn leaves_same_comment_type_and_code_boundaries_unchanged() {
        let input = concat!(
            "// First regular comment.\n",
            "\n",
            "\n",
            "// Second regular comment.\n",
            "let value = 1;\n",
            "/// Item documentation.\n",
        );

        assert_eq!(normalize_comment_spacing(input), input);
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let input = "/* header */\r\n//! Module documentation.\r\n";
        let expected = "/* header */\r\n\r\n//! Module documentation.\r\n";

        assert_eq!(normalize_comment_spacing(input), expected);
    }

    #[test]
    fn reports_the_second_comment_block_line() {
        let diagnostics = CommentSpacingRule {
            severity: Some(Severity::Warning),
        }
        .check("/* header */\n//! Module docs.\n", Path::new("lib.rs"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, 2);
        assert_eq!(diagnostics[0].rule_id, "RC1002");
    }

    #[test]
    fn configured_severity_can_error_or_disable_the_rule() {
        let input = "/* header */\n//! Module docs.\n";
        let error_rule = CommentSpacingRule {
            severity: Some(Severity::Error),
        };
        let diagnostics = error_rule.check(input, Path::new("lib.rs"));
        assert!(matches!(diagnostics[0].severity, Severity::Error));

        let disabled_rule = CommentSpacingRule { severity: None };
        assert!(disabled_rule.check(input, Path::new("lib.rs")).is_empty());
        assert_eq!(disabled_rule.fix(input), input);
    }
}
