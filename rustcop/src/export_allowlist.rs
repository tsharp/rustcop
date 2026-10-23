use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use toml_edit::{value, Array, DocumentMut, Item, Table, Value};

use crate::rules::exports::detect_exports;

pub struct UpdateResult {
    pub added: usize,
    pub total: usize,
}

pub fn update(config_path: &Path, files: &[PathBuf]) -> Result<UpdateResult, String> {
    if !config_path.is_file() {
        return Err(format!(
            "Cannot update export allowlist because {} does not exist",
            config_path.display()
        ));
    }

    let mut detected = BTreeSet::new();
    for file in files
        .iter()
        .filter(|file| file.file_name().and_then(|name| name.to_str()) == Some("lib.rs"))
    {
        let content = std::fs::read_to_string(file)
            .map_err(|error| format!("Could not read {}: {error}", file.display()))?;
        for export in detect_exports(&content, file)? {
            detected.insert(export.name);
        }
    }

    if detected.is_empty() {
        return Err("No library exports or re-exports were detected".to_string());
    }

    let content = std::fs::read_to_string(config_path)
        .map_err(|error| format!("Could not read {}: {error}", config_path.display()))?;
    let (updated, result) = merge_detected_exports(&content, detected)?;
    if updated != content {
        std::fs::write(config_path, updated)
            .map_err(|error| format!("Could not write {}: {error}", config_path.display()))?;
    }

    Ok(result)
}

fn merge_detected_exports(
    content: &str,
    detected: BTreeSet<String>,
) -> Result<(String, UpdateResult), String> {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Could not parse configuration: {error}"))?;

    if !document.contains_key("exports") {
        document["exports"] = Item::Table(Table::new());
    }
    let exports = document["exports"]
        .as_table_mut()
        .ok_or_else(|| "`exports` must be a TOML table".to_string())?;

    let mut allowed = BTreeSet::new();
    if let Some(item) = exports.get("allowed_lib_exports") {
        let array = item
            .as_array()
            .ok_or_else(|| "exports.allowed_lib_exports must be an array".to_string())?;
        for entry in array.iter() {
            let entry = entry
                .as_str()
                .ok_or_else(|| "exports.allowed_lib_exports entries must be strings".to_string())?;
            allowed.insert(entry.to_string());
        }
    }

    let previous_count = allowed.len();
    allowed.extend(detected);

    let mut array = Array::new();
    for export in &allowed {
        let mut export_value = Value::from(export.as_str());
        export_value
            .decor_mut()
            .set_prefix(format!("{newline}    "));
        array.push_formatted(export_value);
    }
    array.set_trailing(newline);
    array.set_trailing_comma(true);
    exports["allowed_lib_exports"] = value(array);

    Ok((
        document.to_string(),
        UpdateResult {
            added: allowed.len().saturating_sub(previous_count),
            total: allowed.len(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_detected_exports_with_existing_entries_and_preserves_comments() {
        let config = concat!(
            "# Keep this comment.\n",
            "[exports]\n",
            "severity = \"error\"\n",
            "allowed_lib_exports = [\"other::client\"]\n",
        );
        let detected = ["gateway::config", "gateway::runtime"]
            .into_iter()
            .map(str::to_string)
            .collect();

        let (updated, result) = merge_detected_exports(config, detected).unwrap();
        let parsed: toml::Value = toml::from_str(&updated).unwrap();
        let entries: Vec<&str> = parsed["exports"]["allowed_lib_exports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry.as_str().unwrap())
            .collect();

        assert_eq!(
            entries,
            ["gateway::config", "gateway::runtime", "other::client"]
        );
        assert!(updated.contains("# Keep this comment."));
        assert_eq!(result.added, 2);
        assert_eq!(result.total, 3);
    }
}
