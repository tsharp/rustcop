use std::collections::BTreeMap;
use std::path::Path;

use crate::config::Config;
use crate::diagnostic::{Diagnostic, Severity};
use crate::rules::Rule;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ImportGroup {
    Std,      // std, core, alloc
    External, // third-party crates
    Internal, // crate, self, super
}

#[derive(Debug, Clone)]
struct ParsedUse {
    visibility: String,
    root: String,
    items: Vec<String>,
    group: ImportGroup,
}

// ---------------------------------------------------------------------------
// Rule implementation
// ---------------------------------------------------------------------------

pub struct ImportFormattingRule {
    group: bool,
    sort: bool,
    merge: bool,
}

impl ImportFormattingRule {
    pub fn new(group: bool, sort: bool, merge: bool) -> Self {
        Self { group, sort, merge }
    }

    pub fn from_config(config: &Config) -> Self {
        Self {
            group: config.imports.group,
            sort: config.imports.sort,
            merge: config.imports.merge,
        }
    }

    fn format_imports(&self, content: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let has_trailing_newline = content.ends_with('\n');

        let Some((region_start, region_end)) = find_use_region(&lines) else {
            return content.to_string();
        };

        let region_lines = &lines[region_start..=region_end];
        let raw_statements = extract_use_statements(region_lines);

        if raw_statements.is_empty() {
            return content.to_string();
        }

        // Parse
        let mut parsed: Vec<ParsedUse> = raw_statements
            .iter()
            .map(|s| parse_use_statement(s))
            .collect();

        // Merge
        if self.merge {
            parsed = merge_imports(parsed);
        }

        // Sort
        if self.sort {
            parsed.sort_by(|a, b| {
                if self.group {
                    a.group.cmp(&b.group).then_with(|| cmp_imports(a, b))
                } else {
                    cmp_imports(a, b)
                }
            });
        }

        // Format the sorted/merged imports
        let formatted = format_parsed_imports(&parsed, self.group);

        // Reconstruct the file
        let mut result = String::new();

        // Lines before the use region
        for line in &lines[..region_start] {
            result.push_str(line);
            result.push('\n');
        }

        // Formatted imports (already has trailing newline)
        result.push_str(&formatted);

        // Lines after the use region – skip leading blank lines to avoid doubles
        let mut after_idx = region_end + 1;
        while after_idx < lines.len() && lines[after_idx].trim().is_empty() {
            after_idx += 1;
        }

        if after_idx < lines.len() {
            result.push('\n'); // single blank line separator
            for i in after_idx..lines.len() {
                result.push_str(lines[i]);
                if i < lines.len() - 1 {
                    result.push('\n');
                }
            }
        }

        // Preserve original trailing newline
        if has_trailing_newline && !result.ends_with('\n') {
            result.push('\n');
        }

        result
    }
}

impl Rule for ImportFormattingRule {
    fn id(&self) -> &str {
        "RC1001"
    }

    fn name(&self) -> &str {
        "ImportFormatting"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        let fixed = self.format_imports(content);
        if fixed != *content {
            vec![Diagnostic {
                rule_id: self.id().to_string(),
                message: "Import statements are not properly formatted. Run `rustcop fix` to auto-fix.".to_string(),
                file: file.to_path_buf(),
                line: 1,
                severity: Severity::Warning,
            }]
        } else {
            vec![]
        }
    }

    fn fix(&self, content: &str) -> String {
        self.format_imports(content)
    }
}

// ---------------------------------------------------------------------------
// Helpers – classification
// ---------------------------------------------------------------------------

fn classify_import(root: &str) -> ImportGroup {
    match root {
        "std" | "core" | "alloc" => ImportGroup::Std,
        "crate" | "self" | "super" => ImportGroup::Internal,
        _ => ImportGroup::External,
    }
}

fn is_use_line(trimmed: &str) -> bool {
    trimmed.starts_with("use ")
        || trimmed.starts_with("pub use ")
        || (trimmed.starts_with("pub(") && trimmed.contains(") use "))
}

// ---------------------------------------------------------------------------
// Helpers – region detection
// ---------------------------------------------------------------------------

/// Find the contiguous block of `use` statements (may include interleaved
/// blank lines and comments). Returns `(first_line, last_line)` inclusive.
fn find_use_region(lines: &[&str]) -> Option<(usize, usize)> {
    let mut first_use: Option<usize> = None;
    let mut last_use_end: usize = 0;
    let mut i = 0;
    let mut brace_depth: i32 = 0;
    let mut found_any_use = false;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Inside a multi-line use statement
        if brace_depth > 0 {
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            last_use_end = i;
            i += 1;
            continue;
        }

        if is_use_line(trimmed) {
            if first_use.is_none() {
                first_use = Some(i);
            }
            found_any_use = true;
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            last_use_end = i;
        } else if found_any_use && (trimmed.is_empty() || trimmed.starts_with("//")) {
            // blank line or comment between uses – keep scanning
        } else if found_any_use {
            break; // end of use region
        }

        i += 1;
    }

    first_use.map(|start| (start, last_use_end))
}

// ---------------------------------------------------------------------------
// Helpers – extraction
// ---------------------------------------------------------------------------

/// Pull individual (possibly multi-line) `use` statements out of the region
/// lines, ignoring blank lines and comments.
fn extract_use_statements(lines: &[&str]) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut brace_depth: i32 = 0;
    let mut in_use = false;

    for line in lines {
        let trimmed = line.trim();

        if !in_use {
            if is_use_line(trimmed) {
                in_use = true;
                current = trimmed.to_string();
                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
                if brace_depth == 0 && trimmed.ends_with(';') {
                    statements.push(current.clone());
                    current.clear();
                    in_use = false;
                }
            }
            // skip comments / blank lines
        } else {
            // continuation of a multi-line use
            current.push(' ');
            current.push_str(trimmed);
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            if brace_depth == 0 {
                statements.push(current.clone());
                current.clear();
                in_use = false;
                brace_depth = 0;
            }
        }
    }

    statements
}

// ---------------------------------------------------------------------------
// Helpers – parsing
// ---------------------------------------------------------------------------

fn parse_use_statement(stmt: &str) -> ParsedUse {
    let trimmed = stmt.trim().trim_end_matches(';').trim();

    // Extract visibility
    let (vis, rest) = extract_visibility(trimmed);

    // Strip `use` keyword
    let path = rest.trim().strip_prefix("use").unwrap_or(rest).trim();

    // Handle absolute paths (`::std::...`)
    let path = path.strip_prefix("::").unwrap_or(path);

    let (root, items) = extract_root_and_items(path);
    let group = classify_import(&root);

    ParsedUse {
        visibility: vis,
        root,
        items,
        group,
    }
}

fn extract_visibility(s: &str) -> (String, &str) {
    if s.starts_with("pub(") {
        let mut depth = 0;
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return (s[..i + 1].to_string(), s[i + 1..].trim());
                    }
                }
                _ => {}
            }
        }
        (String::new(), s) // malformed, treat as no visibility
    } else if s.starts_with("pub ") {
        ("pub".to_string(), &s[4..])
    } else {
        (String::new(), s)
    }
}

fn extract_root_and_items(path: &str) -> (String, Vec<String>) {
    if let Some(pos) = path.find("::") {
        let root = path[..pos].to_string();
        let rest = path[pos + 2..].trim();

        if rest.starts_with('{') && rest.ends_with('}') {
            let inner = &rest[1..rest.len() - 1];
            let items = split_top_level(inner.trim());
            (root, items)
        } else {
            (root, vec![rest.to_string()])
        }
    } else {
        // bare import: `use serde;`
        (path.to_string(), vec![])
    }
}

/// Split a comma-separated list while respecting nested `{ }`.
fn split_top_level(s: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0;
    let mut current = String::new();

    for ch in s.chars() {
        match ch {
            '{' => {
                depth += 1;
                current.push(ch);
            }
            '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                let item = current.trim().to_string();
                if !item.is_empty() {
                    items.push(item);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let item = current.trim().to_string();
    if !item.is_empty() {
        items.push(item);
    }
    items
}

// ---------------------------------------------------------------------------
// Helpers – merging
// ---------------------------------------------------------------------------

fn merge_imports(imports: Vec<ParsedUse>) -> Vec<ParsedUse> {
    // Key: (visibility, root)
    let mut by_key: BTreeMap<(String, String), (ImportGroup, Vec<String>, bool)> = BTreeMap::new();

    for imp in imports {
        let key = (imp.visibility.clone(), imp.root.clone());
        let entry = by_key
            .entry(key)
            .or_insert_with(|| (imp.group, Vec::new(), false));
        if imp.items.is_empty() {
            entry.2 = true; // bare import (`use serde;`)
        } else {
            entry.1.extend(imp.items);
        }
    }

    by_key
        .into_iter()
        .map(|((visibility, root), (group, mut items, has_bare))| {
            if has_bare && !items.is_empty() {
                // The bare import becomes `self` inside the braced list
                items.push("self".to_string());
            }
            // Sort with `self` always first
            items.sort_by(|a, b| match (a.as_str(), b.as_str()) {
                ("self", "self") => std::cmp::Ordering::Equal,
                ("self", _) => std::cmp::Ordering::Less,
                (_, "self") => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            });
            items.dedup();
            ParsedUse {
                visibility,
                root,
                items: if has_bare && items.is_empty() {
                    vec![]
                } else {
                    items
                },
                group,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers – formatting
// ---------------------------------------------------------------------------

fn cmp_imports(a: &ParsedUse, b: &ParsedUse) -> std::cmp::Ordering {
    a.root
        .cmp(&b.root)
        .then_with(|| a.items.join(", ").cmp(&b.items.join(", ")))
}

fn format_parsed_imports(imports: &[ParsedUse], group: bool) -> String {
    let mut result = String::new();
    let mut prev_group: Option<ImportGroup> = None;

    for imp in imports {
        if group {
            if let Some(prev) = prev_group {
                if prev != imp.group {
                    result.push('\n');
                }
            }
        }
        prev_group = Some(imp.group);
        result.push_str(&format_single_import(imp));
        result.push('\n');
    }

    result
}

fn format_single_import(imp: &ParsedUse) -> String {
    let vis = if imp.visibility.is_empty() {
        String::new()
    } else {
        format!("{} ", imp.visibility)
    };

    // Bare import or sole `self` import → `use root;`
    if imp.items.is_empty() || (imp.items.len() == 1 && imp.items[0] == "self") {
        return format!("{vis}use {};", imp.root);
    }

    // Single simple item → `use root::item;`
    if imp.items.len() == 1 {
        return format!("{vis}use {}::{};", imp.root, imp.items[0]);
    }

    // Multiple items – try single line first
    let one_line = format!(
        "{vis}use {}::{{{}}};",
        imp.root,
        imp.items.join(", ")
    );
    if one_line.len() <= 100 {
        return one_line;
    }

    // Multi-line
    let mut s = format!("{vis}use {}::{{\n", imp.root);
    for item in &imp.items {
        s.push_str(&format!("    {item},\n"));
    }
    s.push_str("};");
    s
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grouping_and_sorting() {
        let input = "\
use dashmap::DashMap;
use std::sync::Arc;
use crate::foo;
";
        let rule = ImportFormattingRule::new(true, true, true);
        let output = rule.format_imports(input);
        let expected = "\
use std::sync::Arc;

use dashmap::DashMap;

use crate::foo;
";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_merging_same_crate() {
        let input = "\
use std::sync::Arc;
use std::collections::HashMap;
";
        let rule = ImportFormattingRule::new(true, true, true);
        let output = rule.format_imports(input);
        let expected = "\
use std::{collections::HashMap, sync::Arc};
";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_multiline_use() {
        let input = "\
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};
use std::sync::Arc;
";
        let rule = ImportFormattingRule::new(true, true, true);
        let output = rule.format_imports(input);
        let expected = "\
use std::sync::Arc;

use tokio::{task::JoinHandle, time::{Duration, Instant}};
";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_full_example() {
        let input = "\
use dashmap::DashMap;
use std::sync::Arc;
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};
use tokio_postgres::IsolationLevel;

use crate::context::transaction::{GatewayTransaction, RequestTransactionInfo, TransactionNumber};
use crate::{
    context::{ConnectionContext, SessionId},
    error::{DocumentDBError, ErrorCode, Result},
    postgres::{conn_mgmt::Connection, PgDataClient},
};
";
        let rule = ImportFormattingRule::new(true, true, true);
        let output = rule.format_imports(input);

        // std first, then external, then internal
        assert!(output.starts_with("use std::sync::Arc;\n"));
        assert!(output.contains("\nuse dashmap::DashMap;\n"));
        assert!(output.contains("\nuse crate::"));
    }

    #[test]
    fn test_no_change_when_already_formatted() {
        let input = "\
use std::sync::Arc;

use dashmap::DashMap;

use crate::foo;
";
        let rule = ImportFormattingRule::new(true, true, true);
        let output = rule.format_imports(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_preserves_code_after_imports() {
        let input = "\
use std::sync::Arc;
use dashmap::DashMap;

pub struct Foo;
";
        let rule = ImportFormattingRule::new(true, true, true);
        let output = rule.format_imports(input);
        assert!(output.contains("pub struct Foo;"));
    }

    #[test]
    fn test_pub_use() {
        let input = "\
pub use crate::bar;
use std::sync::Arc;
";
        let rule = ImportFormattingRule::new(true, true, true);
        let output = rule.format_imports(input);
        // pub use and non-pub use should not be merged
        assert!(output.contains("pub use crate::bar;"));
        assert!(output.contains("use std::sync::Arc;"));
    }
}
