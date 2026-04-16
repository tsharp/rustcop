use std::path::Path;

use syn::UseTree;

use crate::{
    config::Config,
    diagnostic::{Diagnostic, Severity},
    rules::Rule,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_LINE_WIDTH: usize = 100;
const INDENT: &str = "    ";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ImportGroup {
    Std,      // std, core, alloc
    External, // third-party crates
    Internal, // crate, self, super
}

/// A normalized, sortable representation of a single `use` statement.
#[derive(Debug, Clone)]
struct NormalizedUse {
    visibility: String,
    tree: UseNode,
    group: ImportGroup,
    leading_lines: Vec<String>,
    original_block: String,
    mergeable: bool,
    skip_format: bool,
}

/// Recursive tree representation of a use path, mirroring syn::UseTree
/// but with sorting/formatting capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UseNode {
    /// `foo::bar` or `foo::{a, b}`
    Path { ident: String, child: Box<UseNode> },
    /// A terminal name, optionally renamed: `HashMap` or `HashMap as Map`
    Name {
        ident: String,
        rename: Option<String>,
    },
    /// `self` optionally renamed
    Slf { rename: Option<String> },
    /// `*`
    Glob,
    /// `{a, b, c}` — a group of sub-trees
    Group { items: Vec<UseNode> },
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
        use crate::config::ImportsConfig;
        let imports = config
            .get_config::<ImportsConfig>("imports")
            .unwrap_or_default();

        Self {
            group: imports.group_imports,
            sort: imports.group_imports, // Using group_imports for now
            merge: imports.import_merge_behaviour == "always",
        }
    }

    fn format_imports(&self, content: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let has_trailing_newline = content.ends_with('\n');

        // Find the use-statement region
        let Some((region_start, region_end)) = find_use_region(&lines) else {
            return content.to_string();
        };

        let parsed = match parse_use_items(&lines[region_start..=region_end]) {
            Some(imports) if !imports.is_empty() => imports,
            _ => return content.to_string(),
        };

        let mut imports = parsed;

        // Merge imports sharing the same root
        if self.merge {
            imports = merge_imports(imports);
        }

        // Sort
        if self.sort {
            for imp in &mut imports {
                sort_use_node(&mut imp.tree);
            }
            imports.sort_by(|a, b| {
                if self.group {
                    a.group
                        .cmp(&b.group)
                        .then_with(|| cmp_use_nodes(&a.tree, &b.tree))
                } else {
                    cmp_use_nodes(&a.tree, &b.tree)
                }
            });
        }

        // Format
        let formatted = format_all_imports(&imports, self.group);

        // Reconstruct the file
        let mut result = String::new();

        // Lines before the use region
        for line in &lines[..region_start] {
            result.push_str(line);
            result.push('\n');
        }

        // Formatted imports
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
                message:
                    "Import statements are not properly formatted. Run `rustcop fix` to auto-fix."
                        .to_string(),
                file: file.to_path_buf(),
                line: 1,
                severity: Severity::Warning,
                suppressed: false,
                suppression_justification: None,
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
// Parsing – syn-based
// ---------------------------------------------------------------------------

/// Parse use statements from region lines while retaining leading trivia.
fn parse_use_items(region_lines: &[&str]) -> Option<Vec<NormalizedUse>> {
    let blocks = parse_import_blocks(region_lines);
    let mut result = Vec::new();

    for block in blocks {
        let item_use: syn::ItemUse = syn::parse_str(&block.use_text).ok()?;
        let vis = format_visibility(&item_use.vis);
        let tree = use_tree_to_node(&item_use.tree);
        let group = classify_node(&tree);

        let has_tag = block.leading_lines.iter().any(|line| {
            let trimmed = line.trim();
            is_attribute_line(trimmed) || is_macro_tag_line(trimmed)
        });
        let is_super = root_ident(&tree) == "super";

        result.push(NormalizedUse {
            visibility: vis,
            tree,
            group,
            leading_lines: block.leading_lines,
            original_block: block.original_block,
            mergeable: !has_tag && !is_super,
            skip_format: has_tag || is_super,
        });
    }

    Some(result)
}

#[derive(Debug)]
struct ImportBlock {
    leading_lines: Vec<String>,
    use_text: String,
    original_block: String,
}

fn parse_import_blocks(region_lines: &[&str]) -> Vec<ImportBlock> {
    let mut blocks = Vec::new();
    let mut pending_leading: Vec<String> = Vec::new();
    let mut i = 0;

    while i < region_lines.len() {
        let line = region_lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if !pending_leading.is_empty() {
                pending_leading.push(line.to_string());
            }
            i += 1;
            continue;
        }

        if is_attachable_line(trimmed) && !is_use_line(trimmed) {
            pending_leading.push(line.to_string());
            i += 1;
            continue;
        }

        if is_use_line(trimmed) {
            let end = consume_use_stmt_end(region_lines, i);
            let use_lines: Vec<String> = region_lines[i..=end]
                .iter()
                .map(|l| l.to_string())
                .collect();
            let use_text = use_lines.join("\n");

            let mut block_lines = pending_leading.clone();
            block_lines.extend(use_lines);

            blocks.push(ImportBlock {
                leading_lines: pending_leading,
                use_text,
                original_block: block_lines.join("\n"),
            });

            pending_leading = Vec::new();
            i = end + 1;
            continue;
        }

        pending_leading.clear();
        i += 1;
    }

    blocks
}

fn consume_use_stmt_end(lines: &[&str], start: usize) -> usize {
    let mut i = start;
    let mut brace_depth = 0i32;

    while i < lines.len() {
        for ch in lines[i].chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
        }

        if brace_depth <= 0 && lines[i].contains(';') {
            return i;
        }

        i += 1;
    }

    lines.len().saturating_sub(1)
}

/// Convert syn::UseTree to our UseNode.
fn use_tree_to_node(tree: &UseTree) -> UseNode {
    match tree {
        UseTree::Path(p) => UseNode::Path {
            ident: p.ident.to_string(),
            child: Box::new(use_tree_to_node(&p.tree)),
        },
        UseTree::Name(n) => UseNode::Name {
            ident: n.ident.to_string(),
            rename: None,
        },
        UseTree::Rename(r) => UseNode::Name {
            ident: r.ident.to_string(),
            rename: Some(r.rename.to_string()),
        },
        UseTree::Glob(_) => UseNode::Glob,
        UseTree::Group(g) => UseNode::Group {
            items: g.items.iter().map(use_tree_to_node).collect(),
        },
    }
}

fn format_visibility(vis: &syn::Visibility) -> String {
    match vis {
        syn::Visibility::Public(_) => "pub".to_string(),
        syn::Visibility::Restricted(r) => {
            let path = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            if r.in_token.is_some() {
                format!("pub(in {path})")
            } else {
                format!("pub({path})")
            }
        }
        syn::Visibility::Inherited => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

fn classify_node(node: &UseNode) -> ImportGroup {
    let root = root_ident(node);
    match root.as_str() {
        "std" | "core" | "alloc" => ImportGroup::Std,
        "crate" | "self" | "super" => ImportGroup::Internal,
        _ => ImportGroup::External,
    }
}

fn root_ident(node: &UseNode) -> String {
    match node {
        UseNode::Path { ident, .. } => ident.clone(),
        UseNode::Name { ident, .. } => ident.clone(),
        UseNode::Slf { .. } => "self".to_string(),
        UseNode::Glob => "*".to_string(),
        UseNode::Group { items } => items.first().map(root_ident).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Region detection (text-based, kept from original)
// ---------------------------------------------------------------------------

fn is_use_line(trimmed: &str) -> bool {
    trimmed.starts_with("use ")
        || trimmed.starts_with("pub use ")
        || (trimmed.starts_with("pub(") && trimmed.contains(") use "))
}

fn is_attribute_line(trimmed: &str) -> bool {
    trimmed.starts_with("#[") || trimmed.starts_with("#![")
}

fn is_macro_tag_line(trimmed: &str) -> bool {
    if trimmed.starts_with("//") || trimmed.starts_with("/*") {
        return false;
    }

    let ident_start = trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');

    ident_start && trimmed.contains('!')
}

fn is_attachable_line(trimmed: &str) -> bool {
    trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || is_attribute_line(trimmed)
        || is_macro_tag_line(trimmed)
}

fn find_use_region(lines: &[&str]) -> Option<(usize, usize)> {
    let mut first_use: Option<usize> = None;
    let mut last_use_end: usize = 0;
    let mut i = 0;
    let mut brace_depth: i32 = 0;
    let mut found_any_use = false;

    while i < lines.len() {
        let trimmed = lines[i].trim();

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
                first_use = Some(backtrack_attached_prefix(lines, i));
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
        } else if found_any_use && (trimmed.is_empty() || is_attachable_line(trimmed)) {
            // blank line or attached trivia between uses – keep scanning
        } else if found_any_use {
            break;
        }

        i += 1;
    }

    first_use.map(|start| (start, last_use_end))
}

fn backtrack_attached_prefix(lines: &[&str], use_line_idx: usize) -> usize {
    let mut start = use_line_idx;
    while start > 0 {
        let prev = lines[start - 1].trim();
        if prev.is_empty() || is_attachable_line(prev) {
            start -= 1;
        } else {
            break;
        }
    }
    start
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

/// Recursively sort all Group nodes in the tree.
fn sort_use_node(node: &mut UseNode) {
    if let UseNode::Path { child, .. } = node {
        sort_use_node(child);
    }
    if let UseNode::Group { items } = node {
        for item in items.iter_mut() {
            sort_use_node(item);
        }
        items.sort_by(cmp_use_nodes);
    }
}

/// Compare two UseNodes for sorting. Matches rustfmt's ordering:
/// self < super < crate < identifiers < glob < groups
/// Within identifiers: snake_case < CamelCase < UPPER_SNAKE_CASE, then lexicographic.
fn cmp_use_nodes(a: &UseNode, b: &UseNode) -> std::cmp::Ordering {
    fn ident_case_category(s: &str) -> u8 {
        if s.starts_with(|c: char| c.is_lowercase()) {
            0 // snake_case
        } else if s.starts_with(|c: char| c.is_uppercase()) {
            if s.chars()
                .all(|c| c.is_uppercase() || c == '_' || c.is_numeric())
            {
                2 // UPPER_SNAKE_CASE
            } else {
                1 // CamelCase
            }
        } else {
            1 // default
        }
    }

    fn sort_key(node: &UseNode) -> (u8, u8, String) {
        match node {
            UseNode::Slf { .. } => (0, 0, String::new()),
            UseNode::Path { ident, child } if ident == "self" => (0, 0, node_sort_suffix(child)),
            UseNode::Path { ident, child } if ident == "super" => (1, 0, node_sort_suffix(child)),
            UseNode::Path { ident, child } if ident == "crate" => (2, 0, node_sort_suffix(child)),
            UseNode::Path { ident, child } => {
                let cat = ident_case_category(ident);
                (3, cat, format!("{ident}::{}", node_sort_suffix(child)))
            }
            UseNode::Name { ident, .. } => {
                let cat = ident_case_category(ident);
                (3, cat, ident.clone())
            }
            UseNode::Glob => (4, 0, String::new()),
            UseNode::Group { .. } => (5, 0, String::new()),
        }
    }

    let (ka, ca, fa) = sort_key(a);
    let (kb, cb, fb) = sort_key(b);
    ka.cmp(&kb)
        .then_with(|| ca.cmp(&cb))
        .then_with(|| fa.cmp(&fb))
}

fn node_sort_suffix(node: &UseNode) -> String {
    match node {
        UseNode::Path { ident, child } => format!("{ident}::{}", node_sort_suffix(child)),
        UseNode::Name { ident, .. } => ident.clone(),
        UseNode::Slf { .. } => "self".to_string(),
        UseNode::Glob => "*".to_string(),
        UseNode::Group { items } => {
            let inner: Vec<String> = items.iter().map(node_sort_suffix).collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

// ---------------------------------------------------------------------------
// Merging
// ---------------------------------------------------------------------------

/// Merge imports that share the same visibility and root path segment.
fn merge_imports(imports: Vec<NormalizedUse>) -> Vec<NormalizedUse> {
    use std::collections::BTreeMap;

    // Group by (visibility, root_ident)
    let mut by_key: BTreeMap<(String, String), Vec<NormalizedUse>> = BTreeMap::new();
    let mut non_mergeable = Vec::new();

    for imp in imports {
        if !imp.mergeable {
            non_mergeable.push(imp);
            continue;
        }
        let root = root_ident(&imp.tree);
        let key = (imp.visibility.clone(), root);
        by_key.entry(key).or_default().push(imp);
    }

    let mut merged: Vec<NormalizedUse> = by_key
        .into_values()
        .map(|group| {
            if group.len() == 1 {
                return group.into_iter().next().unwrap();
            }

            let vis = group[0].visibility.clone();
            let grp = group[0].group;

            // Collect all leaf paths from all trees in this group
            let mut all_children: Vec<UseNode> = Vec::new();
            for imp in &group {
                collect_children_for_merge(&imp.tree, &mut all_children);
            }

            // Deduplicate
            all_children.dedup();

            let root = root_ident(&group[0].tree);

            let tree = if all_children.is_empty() {
                UseNode::Name {
                    ident: root,
                    rename: None,
                }
            } else if all_children.len() == 1 {
                UseNode::Path {
                    ident: root,
                    child: Box::new(all_children.into_iter().next().unwrap()),
                }
            } else {
                UseNode::Path {
                    ident: root,
                    child: Box::new(UseNode::Group {
                        items: all_children,
                    }),
                }
            };

            NormalizedUse {
                visibility: vis,
                tree,
                group: grp,
                leading_lines: Vec::new(),
                original_block: String::new(),
                mergeable: true,
                skip_format: false,
            }
        })
        .collect();

    merged.extend(non_mergeable);
    merged
}

/// Extract the children (everything after the root segment) for merging.
fn collect_children_for_merge(node: &UseNode, out: &mut Vec<UseNode>) {
    match node {
        UseNode::Path { child, .. } => match child.as_ref() {
            UseNode::Group { items } => {
                out.extend(items.iter().cloned());
            }
            other => {
                out.push(other.clone());
            }
        },
        UseNode::Name { rename, .. } => {
            // Bare import like `use serde;` becomes `self`
            out.push(UseNode::Slf {
                rename: rename.clone(),
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Formatting – rustfmt-compatible output
// ---------------------------------------------------------------------------

fn format_all_imports(imports: &[NormalizedUse], group: bool) -> String {
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

        if imp.skip_format {
            result.push_str(&imp.original_block);
            result.push('\n');
            continue;
        }

        for line in &imp.leading_lines {
            result.push_str(line);
            result.push('\n');
        }

        let vis_prefix = if imp.visibility.is_empty() {
            String::new()
        } else {
            format!("{} ", imp.visibility)
        };

        let formatted = format_use_stmt(&imp.tree, &vis_prefix);
        result.push_str(&formatted);
        result.push('\n');
    }

    result
}

/// Format a complete `use` statement from a tree.
fn format_use_stmt(node: &UseNode, vis_prefix: &str) -> String {
    let path_str = format_node_to_path(node);
    let stmt = format!("{vis_prefix}use {path_str};");

    // If it's a simple statement (no braces), just return it
    if !stmt.contains('{') {
        return stmt;
    }

    // If it fits on one line and has no nested braces-within-braces, return it
    let brace_depth = max_brace_depth(node);
    if brace_depth <= 1 && stmt.len() <= MAX_LINE_WIDTH {
        return stmt;
    }

    // Need multi-line formatting
    format_use_stmt_multiline(node, vis_prefix)
}

/// Get the maximum brace nesting depth in a UseNode tree.
fn max_brace_depth(node: &UseNode) -> usize {
    match node {
        UseNode::Group { items } => 1 + items.iter().map(max_brace_depth).max().unwrap_or(0),
        UseNode::Path { child, .. } => max_brace_depth(child),
        _ => 0,
    }
}

/// Format a node as a simple path string (single line, no `use` keyword).
fn format_node_to_path(node: &UseNode) -> String {
    match node {
        UseNode::Name {
            ident,
            rename: None,
        } => ident.clone(),
        UseNode::Name {
            ident,
            rename: Some(alias),
        } => format!("{ident} as {alias}"),
        UseNode::Slf { rename: None } => "self".to_string(),
        UseNode::Slf {
            rename: Some(alias),
        } => format!("self as {alias}"),
        UseNode::Glob => "*".to_string(),
        UseNode::Path { ident, child } => {
            format!("{ident}::{}", format_node_to_path(child))
        }
        UseNode::Group { items } => {
            let inner: Vec<String> = items.iter().map(format_node_to_path).collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Format a use statement with multi-line braces, matching rustfmt behavior.
fn format_use_stmt_multiline(node: &UseNode, vis_prefix: &str) -> String {
    // Collect the path segments leading to the first group
    let mut result = format!("{vis_prefix}use ");
    format_node_multiline(node, &mut result, 0);
    result.push(';');
    result
}

/// Recursively format a node, expanding groups to multiple lines when needed.
fn format_node_multiline(node: &UseNode, out: &mut String, indent_level: usize) {
    match node {
        UseNode::Path { ident, child } => {
            out.push_str(ident);
            out.push_str("::");
            match child.as_ref() {
                UseNode::Group { items } => {
                    format_group_multiline(items, out, indent_level);
                }
                _ => {
                    format_node_multiline(child, out, indent_level);
                }
            }
        }
        UseNode::Group { items } => {
            format_group_multiline(items, out, indent_level);
        }
        // Terminal nodes
        _ => {
            out.push_str(&format_node_to_path(node));
        }
    }
}

/// Format a `{items...}` group, deciding between single-line and multi-line.
/// When multi-line, packs simple items onto lines up to MAX_LINE_WIDTH (like rustfmt).
fn format_group_multiline(items: &[UseNode], out: &mut String, indent_level: usize) {
    let child_indent = INDENT.repeat(indent_level + 1);
    let close_indent = INDENT.repeat(indent_level);

    // Check if any child needs multi-line (has nested groups or would be too long)
    let needs_multiline = items.iter().any(|item| {
        contains_group(item) || {
            let s = format_node_to_path(item);
            child_indent.len() + s.len() + 1 > MAX_LINE_WIDTH
        }
    }) || {
        // Also check if the whole group on one line would be too long
        let inner: Vec<String> = items.iter().map(format_node_to_path).collect();
        let one_line_len = inner.join(", ").len() + 2;
        indent_level * 4 + one_line_len + 10 > MAX_LINE_WIDTH
    };

    if !needs_multiline {
        // Single-line group
        let inner: Vec<String> = items.iter().map(format_node_to_path).collect();
        out.push('{');
        out.push_str(&inner.join(", "));
        out.push('}');
        return;
    }

    // Multi-line group — pack simple items onto lines like rustfmt
    out.push_str("{\n");

    // Separate items into "simple" (no nested groups) and "complex" (has nested groups)
    // but maintain original order. We'll pack simple items on lines.
    let mut i = 0;
    while i < items.len() {
        if contains_group(&items[i]) {
            // Complex item: gets its own indented block
            out.push_str(&child_indent);
            format_node_multiline(&items[i], out, indent_level + 1);
            out.push_str(",\n");
            i += 1;
        } else {
            // Pack consecutive simple items onto lines
            let mut line = child_indent.clone();
            while i < items.len() && !contains_group(&items[i]) {
                let s = format_node_to_path(&items[i]);
                let addition = if line.len() == child_indent.len() {
                    // First item on this line
                    s.clone()
                } else {
                    format!(", {s}")
                };

                // Check if adding this item would exceed line width
                // +1 for the trailing comma
                if line.len() + addition.len() + 1 > MAX_LINE_WIDTH
                    && line.len() > child_indent.len()
                {
                    // This item doesn't fit — flush current line and start new one
                    out.push_str(&line);
                    out.push_str(",\n");
                    line = format!("{child_indent}{s}");
                } else {
                    line.push_str(&addition);
                }
                i += 1;
            }
            // Flush remaining line
            if line.len() > child_indent.len() {
                out.push_str(&line);
                out.push_str(",\n");
            }
        }
    }

    out.push_str(&close_indent);
    out.push('}');
}

/// Check if a UseNode contains any Group (nested braces).
fn contains_group(node: &UseNode) -> bool {
    match node {
        UseNode::Group { .. } => true,
        UseNode::Path { child, .. } => contains_group(child),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bad_format_to_good_format() {
        let input = r#"use criterion::{Criterion, criterion_group, criterion_main};
use documentdb_gateway_core::{
    configuration::{CertInputType, CertificateOptions, DocumentDBSetupConfiguration},
    postgres::{ conn_mgmt::{ run_request_with_retries, Connection, ConnectionPool, ConnectionSource, PgPoolSettings, QueryOptions, RequestOptions, }, ScopedTransaction, },
    requests::request_tracker::RequestTracker,
};"#;

        let expected = r#"use criterion::{criterion_group, criterion_main, Criterion};
use documentdb_gateway_core::{
    configuration::{CertInputType, CertificateOptions, DocumentDBSetupConfiguration},
    postgres::{
        conn_mgmt::{
            run_request_with_retries, Connection, ConnectionPool, ConnectionSource, PgPoolSettings,
            QueryOptions, RequestOptions,
        },
        ScopedTransaction,
    },
    requests::request_tracker::RequestTracker,
};"#;

        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result.trim(), expected.trim(), "\n\nGot:\n{result}");
    }

    #[test]
    fn test_simple_single_line() {
        let input = "use std::collections::HashMap;\n";
        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_sorting_within_braces() {
        let input = "use std::{fmt, collections::HashMap, io};\n";
        let expected = "use std::{collections::HashMap, fmt, io};\n";
        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_grouping() {
        let input = "use crate::foo;\nuse std::io;\nuse serde::Serialize;\n";
        let expected = "use std::io;\n\nuse serde::Serialize;\n\nuse crate::foo;\n";
        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_merge_same_root() {
        let input = "use std::io;\nuse std::fmt;\n";
        let expected = "use std::{fmt, io};\n";
        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_pub_visibility() {
        let input = "pub use serde::{Deserialize, Serialize};\n";
        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_attribute_attached_import_left_alone() {
        let input = concat!(
            "#[cfg(feature = \"io_uring\")]\n",
            "use std::path::PathBuf;\n",
            "use std::collections::HashMap;\n",
        );

        let expected = concat!(
            "use std::collections::HashMap;\n",
            "#[cfg(feature = \"io_uring\")]\n",
            "use std::path::PathBuf;\n",
        );

        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tagged_import_stays_with_group_not_merged() {
        let input = concat!(
            "use serde::Deserialize;\n",
            "#[cfg(feature = \"io_uring\")]\n",
            "use serde::Serialize;\n",
            "use std::fmt;\n",
        );

        let expected = concat!(
            "use std::fmt;\n",
            "\n",
            "use serde::Deserialize;\n",
            "#[cfg(feature = \"io_uring\")]\n",
            "use serde::Serialize;\n",
        );

        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_comment_moves_with_sorted_import() {
        let input = r#"// keep this with serde import
use serde::Serialize;
use std::fmt;
"#;

        let expected = r#"use std::fmt;

// keep this with serde import
use serde::Serialize;
"#;

        let rule = ImportFormattingRule::new(true, true, true);
        let result = rule.format_imports(input);
        assert_eq!(result, expected);
    }
}
