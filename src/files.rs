use std::path::{Path, PathBuf};

use walkdir::WalkDir;

pub fn discover_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            if is_rust_file(path) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            for entry in WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let p = entry.path();
                if p.is_file() && is_rust_file(p) {
                    files.push(p.to_path_buf());
                }
            }
        }
    }
    files.sort();
    files
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().map_or(false, |e| e == "rs")
}
