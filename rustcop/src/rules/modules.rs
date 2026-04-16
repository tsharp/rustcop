use std::path::Path;

use crate::{
    config::{Config, ModulesConfig},
    diagnostic::{Diagnostic, Severity},
    rules::Rule,
};

pub struct ModulesRule {
    enabled: bool,
    severity: Severity,
    preferred_module_order: Vec<String>,
}

impl ModulesRule {
    pub fn from_config(config: &Config) -> Self {
        let modules = config
            .get_config::<ModulesConfig>("modules")
            .unwrap_or_default();

        let enabled = modules.severity != "none";
        let severity = match modules.severity.as_str() {
            "error" => Severity::Error,
            _ => Severity::Warning,
        };

        Self {
            enabled,
            severity,
            preferred_module_order: modules.preferred_module_order,
        }
    }

    fn is_lib_file(file: &Path) -> bool {
        file.file_name().and_then(|n| n.to_str()) == Some("lib.rs")
    }
}

impl Rule for ModulesRule {
    fn id(&self) -> &str {
        "RC3001"
    }

    fn name(&self) -> &str {
        "ModuleRules"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        if !self.enabled || !Self::is_lib_file(file) {
            return vec![];
        }

        let mut diagnostics = Vec::new();

        if self.fix(content) != content {
            diagnostics.push(Diagnostic {
                rule_id: self.id().to_string(),
                message: "Module declarations are not sorted according to configured module order"
                    .to_string(),
                file: file.to_path_buf(),
                line: 1,
                severity: self.severity.clone(),
                suppressed: false,
                suppression_justification: None,
            });
        }

        diagnostics
    }

    fn fix(&self, content: &str) -> String {
        if !self.enabled {
            return content.to_string();
        }

        let lines: Vec<&str> = content.lines().collect();
        let has_trailing_newline = content.ends_with('\n');
        let Some((region_start, region_end)) = find_prelude_region(&lines) else {
            return content.to_string();
        };

        let mut blocks = parse_prelude_blocks(
            &lines[region_start..=region_end],
            &self.preferred_module_order,
        );
        if blocks.len() < 2 {
            return content.to_string();
        }

        let original_order: Vec<(PreludeKind, String, String)> = blocks
            .iter()
            .map(|b| (b.kind, b.sort_group.clone(), b.sort_name.clone()))
            .collect();

        blocks.sort_by(|a, b| {
            a.kind.cmp(&b.kind).then_with(|| match a.kind {
                PreludeKind::Use => a.original_index.cmp(&b.original_index),
                PreludeKind::Mod => a
                    .sort_group
                    .cmp(&b.sort_group)
                    .then_with(|| a.sort_name.cmp(&b.sort_name)),
                PreludeKind::PubUse => a.sort_name.cmp(&b.sort_name),
            })
        });

        let sorted_order: Vec<(PreludeKind, String, String)> = blocks
            .iter()
            .map(|b| (b.kind, b.sort_group.clone(), b.sort_name.clone()))
            .collect();
        if sorted_order == original_order {
            return content.to_string();
        }

        let mut result = String::new();
        for line in &lines[..region_start] {
            result.push_str(line);
            result.push('\n');
        }

        let mut prev_kind: Option<PreludeKind> = None;
        for block in &blocks {
            if let Some(prev) = prev_kind {
                if prev != block.kind {
                    result.push('\n');
                }
            }
            result.push_str(&block.original_block);
            result.push('\n');
            prev_kind = Some(block.kind);
        }

        let mut after_idx = region_end + 1;
        while after_idx < lines.len() && lines[after_idx].trim().is_empty() {
            after_idx += 1;
        }

        if after_idx < lines.len() {
            result.push('\n');
            for i in after_idx..lines.len() {
                result.push_str(lines[i]);
                if i < lines.len() - 1 {
                    result.push('\n');
                }
            }
        }

        if has_trailing_newline && !result.ends_with('\n') {
            result.push('\n');
        }

        result
    }
}

#[derive(Debug, Clone)]
struct ModuleBlock {
    kind: PreludeKind,
    sort_group: String,
    sort_name: String,
    original_index: usize,
    original_block: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreludeKind {
    Use,
    Mod,
    PubUse,
}

fn is_module_decl_line(trimmed: &str) -> bool {
    if !trimmed.ends_with(';') {
        return false;
    }

    let Ok(item_mod) = syn::parse_str::<syn::ItemMod>(trimmed) else {
        return false;
    };

    item_mod.content.is_none()
}

fn is_use_line(trimmed: &str) -> bool {
    trimmed.starts_with("use ") || (trimmed.starts_with("pub(") && trimmed.contains(") use "))
}

fn is_pub_use_line(trimmed: &str) -> bool {
    trimmed.starts_with("pub use ")
}

fn is_prelude_decl_line(trimmed: &str) -> bool {
    is_use_line(trimmed) || is_pub_use_line(trimmed) || is_module_decl_line(trimmed)
}

fn is_attachable_line(trimmed: &str) -> bool {
    trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with("#[")
        || trimmed.starts_with("#![")
}

fn find_prelude_region(lines: &[&str]) -> Option<(usize, usize)> {
    let mut first_decl: Option<usize> = None;
    let mut last_decl: usize = 0;
    let mut found_any = false;
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if is_prelude_decl_line(trimmed) {
            if first_decl.is_none() {
                first_decl = Some(backtrack_attached_prefix(lines, i));
            }
            last_decl = if is_module_decl_line(trimmed) {
                i
            } else {
                consume_stmt_end(lines, i)
            };
            found_any = true;
        } else if found_any && (trimmed.is_empty() || is_attachable_line(trimmed)) {
            i += 1;
            continue;
        } else if found_any {
            break;
        }

        i += 1;
    }

    first_decl.map(|start| (start, last_decl))
}

fn backtrack_attached_prefix(lines: &[&str], decl_idx: usize) -> usize {
    let mut start = decl_idx;
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

fn parse_prelude_blocks(
    region_lines: &[&str],
    preferred_module_order: &[String],
) -> Vec<ModuleBlock> {
    let mut blocks = Vec::new();
    let mut pending_leading: Vec<String> = Vec::new();
    let mut i = 0;
    let mut block_index = 0usize;

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

        if is_attachable_line(trimmed) && !is_prelude_decl_line(trimmed) {
            pending_leading.push(line.to_string());
            i += 1;
            continue;
        }

        if is_prelude_decl_line(trimmed) {
            let end = if is_module_decl_line(trimmed) {
                i
            } else {
                consume_stmt_end(region_lines, i)
            };

            let stmt_lines: Vec<String> = region_lines[i..=end]
                .iter()
                .map(|line| (*line).to_string())
                .collect();

            let mut block_lines = pending_leading.clone();
            block_lines.extend(stmt_lines.clone());

            if is_module_decl_line(trimmed) {
                let Ok(item_mod) = syn::parse_str::<syn::ItemMod>(trimmed) else {
                    pending_leading.clear();
                    i = end + 1;
                    continue;
                };

                let category = module_category(&item_mod.vis, preferred_module_order);
                blocks.push(ModuleBlock {
                    kind: PreludeKind::Mod,
                    sort_group: category,
                    sort_name: item_mod.ident.to_string(),
                    original_index: block_index,
                    original_block: block_lines.join("\n"),
                });
            } else if is_pub_use_line(trimmed) {
                blocks.push(ModuleBlock {
                    kind: PreludeKind::PubUse,
                    sort_group: String::new(),
                    sort_name: stmt_lines.join("\n"),
                    original_index: block_index,
                    original_block: block_lines.join("\n"),
                });
            } else {
                blocks.push(ModuleBlock {
                    kind: PreludeKind::Use,
                    sort_group: String::new(),
                    sort_name: stmt_lines.join("\n"),
                    original_index: block_index,
                    original_block: block_lines.join("\n"),
                });
            }

            block_index += 1;
            pending_leading.clear();
            i = end + 1;
            continue;
        }

        pending_leading.clear();
        i += 1;
    }

    blocks
}

fn consume_stmt_end(lines: &[&str], start: usize) -> usize {
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

fn module_category(vis: &syn::Visibility, preferred_module_order: &[String]) -> String {
    match vis {
        syn::Visibility::Inherited => "local".to_string(),
        syn::Visibility::Public(_) => "local".to_string(),
        syn::Visibility::Restricted(r) => {
            let path = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>();
            if path.first().is_some_and(|p| p == "crate") {
                if r.in_token.is_some() {
                    if preferred_module_order.iter().any(|v| v == "in_crate") {
                        "in_crate".to_string()
                    } else {
                        "crate".to_string()
                    }
                } else {
                    "crate".to_string()
                }
            } else if path.first().is_some_and(|p| p == "super") {
                "super".to_string()
            } else {
                "local".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sorts_module_declarations() {
        let input = concat!(
            "use std::path::PathBuf;\n",
            "pub mod suppression;\n",
            "pub mod config;\n",
            "pub mod rules;\n",
            "pub use rustcop_macros::*;\n",
        );

        let rule = ModulesRule {
            enabled: true,
            severity: Severity::Error,
            preferred_module_order: vec!["local".to_string(), "crate".to_string()],
        };

        let fixed = rule.fix(input);
        let expected = concat!(
            "use std::path::PathBuf;\n",
            "\n",
            "pub mod config;\n",
            "pub mod rules;\n",
            "pub mod suppression;\n",
            "\n",
            "pub use rustcop_macros::*;\n",
        );
        assert_eq!(fixed, expected);
    }

    #[test]
    fn test_pub_use_is_own_sorted_group() {
        let input = concat!(
            "pub use zed::Thing;\n",
            "use std::fmt;\n",
            "pub mod config;\n",
            "pub use alpha::Thing;\n",
        );

        let rule = ModulesRule {
            enabled: true,
            severity: Severity::Error,
            preferred_module_order: vec!["local".to_string(), "crate".to_string()],
        };

        let fixed = rule.fix(input);
        let expected = concat!(
            "use std::fmt;\n",
            "\n",
            "pub mod config;\n",
            "\n",
            "pub use alpha::Thing;\n",
            "pub use zed::Thing;\n",
        );
        assert_eq!(fixed, expected);
    }

    #[test]
    fn test_use_group_order_is_preserved() {
        let input = concat!(
            "use std::fmt;\n",
            "use clap::Parser;\n",
            "use config::Config;\n",
            "pub mod config;\n",
            "pub use zed::Thing;\n",
            "pub use alpha::Thing;\n",
        );

        let rule = ModulesRule {
            enabled: true,
            severity: Severity::Error,
            preferred_module_order: vec!["local".to_string(), "crate".to_string()],
        };

        let fixed = rule.fix(input);
        let expected = concat!(
            "use std::fmt;\n",
            "use clap::Parser;\n",
            "use config::Config;\n",
            "\n",
            "pub mod config;\n",
            "\n",
            "pub use alpha::Thing;\n",
            "pub use zed::Thing;\n",
        );
        assert_eq!(fixed, expected);
    }
}
