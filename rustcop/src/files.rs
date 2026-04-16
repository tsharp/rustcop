use std::path::{Path, PathBuf};

use walkdir::WalkDir;

pub fn discover_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            // Direct file reference - scan it
            if is_rust_file(path) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            let workspace_member_srcs = workspace_member_src_dirs(path);
            if !workspace_member_srcs.is_empty() {
                for member_src in workspace_member_srcs {
                    collect_rust_files(&member_src, &mut files);
                }
                continue;
            }

            // Directory reference - only scan src/ by default
            let scan_path = if should_restrict_to_src(path) {
                // Check if src/ exists under this directory
                let src_path = path.join("src");
                if src_path.is_dir() {
                    src_path
                } else {
                    // No src/ directory, skip this path
                    continue;
                }
            } else {
                // Already inside src/ or a specific subdirectory - scan it
                path.clone()
            };

            collect_rust_files(&scan_path, &mut files);
        }
    }
    files.sort();
    files.dedup();
    files
}

fn collect_rust_files(scan_path: &Path, files: &mut Vec<PathBuf>) {
    for entry in WalkDir::new(scan_path).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() && is_rust_file(p) {
            files.push(p.to_path_buf());
        }
    }
}

fn workspace_member_src_dirs(path: &Path) -> Vec<PathBuf> {
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return vec![];
    }

    let content = match std::fs::read_to_string(&cargo_toml) {
        Ok(content) => content,
        Err(_) => return vec![],
    };

    let parsed: toml::Value = match toml::from_str(&content) {
        Ok(value) => value,
        Err(_) => return vec![],
    };

    let members = match parsed
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
    {
        Some(members) => members,
        None => return vec![],
    };

    let mut src_dirs = Vec::new();
    for member in members {
        let Some(member_path) = member.as_str() else {
            continue;
        };

        if member_path.contains('*') || member_path.contains('?') {
            continue;
        }

        let member_src = path.join(member_path).join("src");
        if member_src.is_dir() {
            src_dirs.push(member_src);
        }
    }

    src_dirs
}

/// Determine if we should restrict scanning to src/ subdirectory
/// Returns true for workspace root directories (., .., or paths not starting with src/)
fn should_restrict_to_src(path: &Path) -> bool {
    // Get the last component of the path
    let last_component = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // If it's "." or ".." or has no components, restrict to src
    if last_component.is_empty() || last_component == "." || last_component == ".." {
        return true;
    }

    // If the path ends with "src" or contains "src/" in it, don't restrict
    if last_component == "src" {
        return false;
    }

    // Check if any ancestor is named "src"
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if s == "src" {
                return false;
            }
        }
    }

    // Otherwise, restrict to src/ (this is a workspace root or non-src directory)
    true
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "rs")
}
