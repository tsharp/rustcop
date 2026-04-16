use std::{collections::HashSet, path::Path};

use crate::{
    config::{Config, ExportsConfig},
    diagnostic::{Diagnostic, Severity},
    rules::Rule,
};

pub struct ExportsRule {
    enabled: bool,
    severity: Severity,
    allowed_lib_exports: HashSet<String>,
}

impl ExportsRule {
    pub fn from_config(config: &Config) -> Self {
        let exports = config
            .get_config::<ExportsConfig>("exports")
            .unwrap_or_default();

        let enabled = exports.severity != "none";
        let severity = match exports.severity.as_str() {
            "error" => Severity::Error,
            _ => Severity::Warning,
        };

        Self {
            enabled,
            severity,
            allowed_lib_exports: exports.allowed_lib_exports.into_iter().collect(),
        }
    }

    fn is_lib_file(file: &Path) -> bool {
        file.file_name().and_then(|n| n.to_str()) == Some("lib.rs")
    }
}

impl Rule for ExportsRule {
    fn id(&self) -> &str {
        "RC3002"
    }

    fn name(&self) -> &str {
        "ExportRules"
    }

    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        if !self.enabled || !Self::is_lib_file(file) {
            return vec![];
        }

        if self.allowed_lib_exports.is_empty() {
            return vec![];
        }

        let lines: Vec<&str> = content.lines().collect();
        let mut diagnostics = Vec::new();
        let mut i = 0usize;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            if trimmed.starts_with("pub mod ") {
                if let Ok(item_mod) = syn::parse_str::<syn::ItemMod>(trimmed) {
                    let name = item_mod.ident.to_string();
                    if !self.allowed_lib_exports.contains(&name) {
                        diagnostics.push(Diagnostic {
                            rule_id: self.id().to_string(),
                            message: format!(
                                "Module `{}` is exported from lib.rs but is not in exports.allowed_lib_exports",
                                name
                            ),
                            file: file.to_path_buf(),
                            line: i + 1,
                            severity: self.severity.clone(),
                            suppressed: false,
                            suppression_justification: None,
                        });
                    }
                }
                i += 1;
                continue;
            }

            if trimmed.starts_with("pub use ") {
                let end = consume_stmt_end(&lines, i);
                let stmt = lines[i..=end].join("\n");

                if let Ok(item_use) = syn::parse_str::<syn::ItemUse>(&stmt) {
                    let mut modules = Vec::new();
                    collect_reexported_modules(&item_use.tree, &mut modules);

                    for module in modules {
                        if !self.allowed_lib_exports.contains(module.as_str()) {
                            diagnostics.push(Diagnostic {
                                rule_id: self.id().to_string(),
                                message: format!(
                                    "Module `{}` is re-exported from lib.rs but is not in exports.allowed_lib_exports",
                                    module
                                ),
                                file: file.to_path_buf(),
                                line: i + 1,
                                severity: self.severity.clone(),
                                suppressed: false,
                                suppression_justification: None,
                            });
                        }
                    }
                }

                i = end + 1;
                continue;
            }

            i += 1;
        }

        diagnostics
    }

    fn fix(&self, content: &str) -> String {
        content.to_string()
    }
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

fn collect_reexported_modules(tree: &syn::UseTree, out: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => {
            let ident = path.ident.to_string();
            if ident == "crate" || ident == "self" || ident == "super" {
                collect_reexported_modules(&path.tree, out);
            } else {
                out.push(ident);
            }
        }
        syn::UseTree::Name(name) => out.push(name.ident.to_string()),
        syn::UseTree::Rename(rename) => out.push(rename.ident.to_string()),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_reexported_modules(item, out);
            }
        }
        syn::UseTree::Glob(_) => out.push("*".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reports_disallowed_pub_mod_exports() {
        let content = concat!("pub mod config;\n", "pub mod secret;\n",);

        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: ["config".to_string()].into_iter().collect(),
        };

        let diags = rule.check(content, Path::new("lib.rs"));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("secret"));
    }

    #[test]
    fn test_reports_disallowed_pub_use_reexports() {
        let content = concat!(
            "pub use config::Config;\n",
            "pub use secret::Hidden;\n",
            "pub use crate::{config, secret as alias};\n",
        );

        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: ["config".to_string()].into_iter().collect(),
        };

        let diags = rule.check(content, Path::new("lib.rs"));
        assert!(diags.iter().any(|d| d.message.contains("secret")));
    }

    #[test]
    fn test_empty_allowlist_is_unconstrained() {
        let content = concat!("pub mod secret;\n", "pub use secret::Hidden;\n",);

        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: HashSet::new(),
        };

        let diags = rule.check(content, Path::new("lib.rs"));
        assert!(diags.is_empty());
    }

    #[test]
    fn test_non_lib_file_is_ignored() {
        let content = "pub mod secret;\n";
        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: ["config".to_string()].into_iter().collect(),
        };

        let diags = rule.check(content, Path::new("main.rs"));
        assert!(diags.is_empty());
    }
}
