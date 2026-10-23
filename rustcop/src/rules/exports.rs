use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

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

pub(crate) struct DetectedExport {
    pub name: String,
    pub line: usize,
    pub reexported: bool,
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
            return vec![Diagnostic {
                rule_id: self.id().to_string(),
                message: "exports.allowed_lib_exports must not be empty; run `rustcop fix --update-export-allowlist` to add detected exports"
                    .to_string(),
                file: file.to_path_buf(),
                line: 1,
                severity: self.severity.clone(),
                suppressed: false,
                suppression_justification: None,
            }];
        }

        let mut invalid_entries: Vec<&str> = self
            .allowed_lib_exports
            .iter()
            .map(String::as_str)
            .filter(|entry| !is_qualified_export(entry))
            .collect();
        if !invalid_entries.is_empty() {
            invalid_entries.sort_unstable();
            return vec![Diagnostic {
                rule_id: self.id().to_string(),
                message: format!(
                    "exports.allowed_lib_exports entries must use `crate_name::export` format; invalid entries: {}",
                    invalid_entries.join(", ")
                ),
                file: file.to_path_buf(),
                line: 1,
                severity: self.severity.clone(),
                suppressed: false,
                suppression_justification: None,
            }];
        }

        let exports = match detect_exports(content, file) {
            Ok(exports) => exports,
            Err(error) => {
                return vec![Diagnostic {
                    rule_id: self.id().to_string(),
                    message: error,
                    file: file.to_path_buf(),
                    line: 1,
                    severity: self.severity.clone(),
                    suppressed: false,
                    suppression_justification: None,
                }];
            }
        };

        self.check_detected_exports(file, exports)
    }

    fn fix(&self, content: &str) -> String {
        content.to_string()
    }
}

impl ExportsRule {
    #[cfg(test)]
    fn check_exports(&self, content: &str, file: &Path, crate_name: &str) -> Vec<Diagnostic> {
        self.check_detected_exports(file, detect_exports_for_crate(content, crate_name))
    }

    fn check_detected_exports(&self, file: &Path, exports: Vec<DetectedExport>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for export in exports {
            if !self.allowed_lib_exports.contains(&export.name) {
                let action = if export.reexported {
                    "re-exported"
                } else {
                    "exported"
                };
                diagnostics.push(Diagnostic {
                    rule_id: self.id().to_string(),
                    message: format!(
                        "Module `{}` is {action} from lib.rs but is not in exports.allowed_lib_exports",
                        export.name
                    ),
                    file: file.to_path_buf(),
                    line: export.line,
                    severity: self.severity.clone(),
                    suppressed: false,
                    suppression_justification: None,
                });
            }
        }
        diagnostics
    }
}

pub(crate) fn detect_exports(content: &str, file: &Path) -> Result<Vec<DetectedExport>, String> {
    let crate_name = crate_name_for_file(file)?;
    Ok(detect_exports_for_crate(content, &crate_name))
}

fn detect_exports_for_crate(content: &str, crate_name: &str) -> Vec<DetectedExport> {
    let lines: Vec<&str> = content.lines().collect();
    let mut exports: BTreeMap<String, (usize, bool)> = BTreeMap::new();
    let mut i = 0usize;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("pub mod ") {
            if let Ok(item_mod) = syn::parse_str::<syn::ItemMod>(trimmed) {
                exports
                    .entry(format!("{crate_name}::{}", item_mod.ident))
                    .or_insert((i + 1, false));
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
                    exports
                        .entry(format!("{crate_name}::{module}"))
                        .or_insert((i + 1, true));
                }
            }

            i = end + 1;
            continue;
        }

        i += 1;
    }

    exports
        .into_iter()
        .map(|(name, (line, reexported))| DetectedExport {
            name,
            line,
            reexported,
        })
        .collect()
}

fn is_qualified_export(entry: &str) -> bool {
    let mut segments = entry.split("::");
    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(crate_name), Some(export), None) if !crate_name.is_empty() && !export.is_empty()
    )
}

fn crate_name_for_file(file: &Path) -> Result<String, String> {
    let absolute_file = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("Could not resolve current directory: {error}"))?
            .join(file)
    };

    for directory in absolute_file.parent().into_iter().flat_map(Path::ancestors) {
        let manifest = directory.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }

        let content = std::fs::read_to_string(&manifest)
            .map_err(|error| format!("Could not read {}: {error}", manifest.display()))?;
        let parsed: toml::Value = toml::from_str(&content)
            .map_err(|error| format!("Could not parse {}: {error}", manifest.display()))?;
        let crate_name = parsed
            .get("lib")
            .and_then(|lib| lib.get("name"))
            .and_then(toml::Value::as_str)
            .or_else(|| {
                parsed
                    .get("package")
                    .and_then(|package| package.get("name"))
                    .and_then(toml::Value::as_str)
            });

        if let Some(crate_name) = crate_name {
            return Ok(crate_name.replace('-', "_"));
        }
    }

    Err(format!(
        "Could not determine the crate name for {}; no package Cargo.toml was found",
        file.display()
    ))
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
            allowed_lib_exports: ["gateway::config".to_string()].into_iter().collect(),
        };

        let diags = rule.check_exports(content, Path::new("lib.rs"), "gateway");
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
            allowed_lib_exports: ["gateway::config".to_string()].into_iter().collect(),
        };

        let diags = rule.check_exports(content, Path::new("lib.rs"), "gateway");
        assert!(diags.iter().any(|d| d.message.contains("secret")));
    }

    #[test]
    fn test_empty_allowlist_is_an_error() {
        let content = concat!("pub mod secret;\n", "pub use secret::Hidden;\n",);

        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: HashSet::new(),
        };

        let diags = rule.check(content, Path::new("lib.rs"));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("--update-export-allowlist"));
    }

    #[test]
    fn test_non_lib_file_is_ignored() {
        let content = "pub mod secret;\n";
        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: ["gateway::config".to_string()].into_iter().collect(),
        };

        let diags = rule.check(content, Path::new("main.rs"));
        assert!(diags.is_empty());
    }

    #[test]
    fn test_requires_crate_qualified_allowlist_entries() {
        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: ["config".to_string()].into_iter().collect(),
        };

        let diags = rule.check("pub mod config;\n", Path::new("lib.rs"));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("crate_name::export"));
        assert!(diags[0].message.contains("config"));
    }

    #[test]
    fn test_allowlist_is_scoped_to_crate() {
        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: ["other_crate::config".to_string()].into_iter().collect(),
        };

        let diags = rule.check_exports("pub mod config;\n", Path::new("lib.rs"), "gateway");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("gateway::config"));
    }

    #[test]
    fn test_resolves_crate_name_from_manifest() {
        assert_eq!(
            crate_name_for_file(Path::new("rustcop/src/lib.rs")).unwrap(),
            "rustcop"
        );
    }

    #[test]
    fn test_detects_modules_and_reexport_source_roots() {
        let content = concat!(
            "pub mod config;\n",
            "pub use runtime::benchmarks as runtime_benchmarks;\n",
            "pub use crate::{config, telemetry::Telemetry};\n",
            "pub use macros::*;\n",
        );

        let exports = detect_exports_for_crate(content, "gateway");
        let names: Vec<&str> = exports.iter().map(|export| export.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "gateway::config",
                "gateway::macros",
                "gateway::runtime",
                "gateway::telemetry",
            ]
        );
        assert!(exports
            .iter()
            .find(|export| export.name == "gateway::runtime")
            .is_some_and(|export| export.reexported));
    }
}
