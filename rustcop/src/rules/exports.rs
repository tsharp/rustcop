use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

use syn::spanned::Spanned;

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
        self.check_detected_exports(file, detect_exports_for_crate(content, crate_name).unwrap())
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
    detect_exports_for_crate(content, &crate_name)
}

fn detect_exports_for_crate(
    content: &str,
    crate_name: &str,
) -> Result<Vec<DetectedExport>, String> {
    let file = syn::parse_file(content)
        .map_err(|error| format!("Could not parse library exports: {error}"))?;
    let local_modules: HashSet<String> = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module.ident.to_string()),
            _ => None,
        })
        .collect();
    let mut exports: BTreeMap<String, (usize, bool)> = BTreeMap::new();

    for item in file.items {
        match item {
            syn::Item::Mod(item_mod) if matches!(item_mod.vis, syn::Visibility::Public(_)) => {
                exports
                    .entry(format!("{crate_name}::{}", item_mod.ident))
                    .or_insert((item_mod.span().start().line, false));
            }
            syn::Item::Use(item_use) if matches!(item_use.vis, syn::Visibility::Public(_)) => {
                let mut paths = Vec::new();
                collect_reexported_paths(
                    &item_use.tree,
                    crate_name,
                    &local_modules,
                    &[],
                    true,
                    &mut paths,
                );
                for path in paths {
                    exports
                        .entry(path)
                        .or_insert((item_use.span().start().line, true));
                }
            }
            _ => {}
        }
    }

    Ok(exports
        .into_iter()
        .map(|(name, (line, reexported))| DetectedExport {
            name,
            line,
            reexported,
        })
        .collect())
}

fn collect_reexported_paths(
    tree: &syn::UseTree,
    crate_name: &str,
    local_modules: &HashSet<String>,
    prefix: &[String],
    at_root: bool,
    out: &mut Vec<String>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            let ident = path.ident.to_string();
            let mut next = prefix.to_vec();
            if at_root && (ident == "crate" || ident == "self") {
                next.push(crate_name.to_string());
            } else if at_root && local_modules.contains(&ident) {
                next.push(crate_name.to_string());
                next.push(ident);
            } else {
                next.push(ident);
            }
            collect_reexported_paths(&path.tree, crate_name, local_modules, &next, false, out);
        }
        syn::UseTree::Name(name) => {
            let ident = name.ident.to_string();
            let mut path = prefix.to_vec();
            if ident != "self" {
                if at_root && local_modules.contains(&ident) {
                    path.push(crate_name.to_string());
                }
                path.push(ident);
            }
            if !path.is_empty() {
                out.push(path.join("::"));
            }
        }
        syn::UseTree::Rename(rename) => {
            let ident = rename.ident.to_string();
            let mut path = prefix.to_vec();
            if ident != "self" {
                if at_root && local_modules.contains(&ident) {
                    path.push(crate_name.to_string());
                }
                path.push(ident);
            }
            if !path.is_empty() {
                out.push(format!("{} as {}", path.join("::"), rename.rename));
            }
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_reexported_paths(item, crate_name, local_modules, prefix, at_root, out);
            }
        }
        syn::UseTree::Glob(_) => {
            let mut path = prefix.to_vec();
            path.push("*".to_string());
            out.push(path.join("::"));
        }
    }
}

fn is_qualified_export(entry: &str) -> bool {
    syn::parse_str::<syn::ItemUse>(&format!("use {entry};"))
        .ok()
        .and_then(|item| use_tree_segment_count(&item.tree))
        .is_some_and(|segments| segments >= 2)
}

fn use_tree_segment_count(tree: &syn::UseTree) -> Option<usize> {
    match tree {
        syn::UseTree::Path(path) => use_tree_segment_count(&path.tree).map(|count| count + 1),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => Some(1),
        syn::UseTree::Group(_) => None,
    }
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
            "pub use crate_a::thing;\n",
            "pub use crate_b::thing;\n",
            "pub use crate_c::thing as new_thing;\n",
        );

        let rule = ExportsRule {
            enabled: true,
            severity: Severity::Error,
            allowed_lib_exports: ["crate_a::thing".to_string()].into_iter().collect(),
        };

        let diags = rule.check_exports(content, Path::new("lib.rs"), "gateway");
        assert_eq!(diags.len(), 2);
        assert!(diags
            .iter()
            .any(|diagnostic| diagnostic.message.contains("crate_b::thing")));
        assert!(diags
            .iter()
            .any(|diagnostic| diagnostic.message.contains("crate_c::thing as new_thing")));
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
    fn test_accepts_exact_alias_and_glob_allowlist_entries() {
        assert!(is_qualified_export("crate_c::thing as new_thing"));
        assert!(is_qualified_export("external_macros::*"));
        assert!(!is_qualified_export("crate_a::{thing, other}"));
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
    fn test_detects_exact_module_and_reexport_paths() {
        let content = concat!(
            "mod runtime;\n",
            "pub mod config;\n",
            "pub use runtime::benchmarks as runtime_benchmarks;\n",
            "pub use crate::{config, telemetry::Telemetry};\n",
            "pub use macros::*;\n",
        );

        let exports = detect_exports_for_crate(content, "gateway").unwrap();
        let names: Vec<&str> = exports.iter().map(|export| export.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "gateway::config",
                "gateway::runtime::benchmarks as runtime_benchmarks",
                "gateway::telemetry::Telemetry",
                "macros::*",
            ]
        );
        assert!(exports
            .iter()
            .find(|export| { export.name == "gateway::runtime::benchmarks as runtime_benchmarks" })
            .is_some_and(|export| export.reexported));
    }
}
