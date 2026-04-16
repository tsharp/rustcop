use std::path::{Path, PathBuf};

use globset::{Glob, GlobSetBuilder};
use serde::Deserialize;

/// Generic configuration container that stores raw TOML data
/// and allows deserializing specific sections on demand.
#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) raw: toml::Value,
    config_dir: Option<PathBuf>,
}

/// A raw configuration file with metadata
#[derive(Debug)]
struct RawConfig {
    raw: toml::Value,
    path: PathBuf,
    #[allow(dead_code)]
    is_root: bool,
}

/// Override configuration block
#[derive(Debug, Deserialize, Clone)]
struct Override {
    files: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(flatten)]
    config: toml::Value,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let raw: toml::Value = toml::from_str(&content)?;

        // Validate version
        Self::validate_version(&raw)?;

        let config_dir = path.parent().map(|p| p.to_path_buf());
        Ok(Config { raw, config_dir })
    }

    /// Create an empty configuration (all sections will use defaults)
    pub fn empty() -> Self {
        Config {
            raw: toml::Value::Table(Default::default()),
            config_dir: None,
        }
    }

    /// Resolve configuration for a specific source file (hierarchical discovery + overrides)
    ///
    /// This implements the full resolution algorithm from the spec:
    /// 1. Discover rustcop.toml files by walking upward
    /// 2. Stop at root = true
    /// 3. Apply configs from outermost → innermost
    /// 4. Apply matching overrides
    pub fn resolve_for_file(source_file: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        // Discover all config files
        let config_files = Self::discover_configs(source_file)?;

        if config_files.is_empty() {
            return Ok(Self::empty());
        }

        // Start with built-in defaults (empty config)
        let mut merged = toml::Value::Table(toml::map::Map::new());
        let mut final_config_dir = None;

        // Apply configs from outermost → innermost
        for raw_config in config_files {
            if final_config_dir.is_none() {
                final_config_dir = raw_config.path.parent().map(|p| p.to_path_buf());
            }
            Self::merge_tables(&mut merged, &raw_config.raw);
        }

        let mut config = Config {
            raw: merged,
            config_dir: final_config_dir,
        };

        // Apply matching overrides
        config.apply_overrides_for_file(source_file)?;

        Ok(config)
    }

    /// Discover rustcop.toml files by walking upward from the source file
    fn discover_configs(source_file: &Path) -> Result<Vec<RawConfig>, Box<dyn std::error::Error>> {
        let mut configs = Vec::new();
        let mut current_dir = source_file.parent();

        while let Some(dir) = current_dir {
            let config_path = dir.join("rustcop.toml");

            if config_path.exists() {
                let content = std::fs::read_to_string(&config_path)?;
                let raw: toml::Value = toml::from_str(&content)?;

                // Validate version
                Self::validate_version(&raw)?;

                let is_root = raw.get("root").and_then(|v| v.as_bool()).unwrap_or(false);

                configs.push(RawConfig {
                    raw,
                    path: config_path,
                    is_root,
                });

                if is_root {
                    break;
                }
            }

            current_dir = dir.parent();
        }

        // Reverse so outermost is first
        configs.reverse();
        Ok(configs)
    }

    /// Validate the version field
    fn validate_version(raw: &toml::Value) -> Result<(), Box<dyn std::error::Error>> {
        match raw.get("version") {
            Some(toml::Value::Integer(1)) => Ok(()),
            Some(toml::Value::Integer(v)) => Err(format!(
                "Unsupported config version: {}. Only version 1 is supported.",
                v
            )
            .into()),
            Some(_) => Err("Invalid version field: must be an integer".into()),
            None => {
                // Version is optional for now, but we warn
                eprintln!("warning: rustcop.toml is missing 'version' field. Add 'version = 1' to the config.");
                Ok(())
            }
        }
    }

    /// Merge two TOML tables according to spec merge semantics:
    /// - Scalars: replaced
    /// - Arrays: replaced entirely
    /// - Tables: merged recursively
    fn merge_tables(base: &mut toml::Value, override_val: &toml::Value) {
        use toml::Value;

        match (base, override_val) {
            (Value::Table(base_map), Value::Table(override_map)) => {
                for (key, override_value) in override_map {
                    if let Some(base_value) = base_map.get_mut(key) {
                        // Recursively merge if both are tables
                        if matches!(base_value, Value::Table(_))
                            && matches!(override_value, Value::Table(_))
                        {
                            Self::merge_tables(base_value, override_value);
                        } else {
                            // Replace (scalars and arrays)
                            *base_value = override_value.clone();
                        }
                    } else {
                        base_map.insert(key.clone(), override_value.clone());
                    }
                }
            }
            _ => {
                // This shouldn't happen if we're only merging root tables
            }
        }
    }

    /// Apply overrides that match the given source file
    fn apply_overrides_for_file(
        &mut self,
        source_file: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Extract overrides array
        let overrides = match self.raw.get("overrides") {
            Some(toml::Value::Array(arr)) => arr.clone(),
            _ => return Ok(()), // No overrides
        };

        // Get the config directory for relative path resolution
        let config_dir = self
            .config_dir
            .as_ref()
            .ok_or("Cannot apply overrides without config directory")?;

        // Make source_file relative to config_dir
        let relative_source = source_file.strip_prefix(config_dir).unwrap_or(source_file);

        // Apply each matching override in order
        for override_val in overrides {
            if let Ok(override_config) = override_val.clone().try_into::<Override>() {
                if Self::matches_override(&override_config, relative_source)? {
                    // Apply this override by merging its config
                    let mut override_config_val = override_config.config;

                    // Remove the 'files' and 'exclude' fields that were captured
                    if let toml::Value::Table(ref mut map) = override_config_val {
                        map.remove("files");
                        map.remove("exclude");
                    }

                    Self::merge_tables(&mut self.raw, &override_config_val);
                }
            }
        }

        Ok(())
    }

    /// Check if a file matches an override's patterns
    fn matches_override(
        override_config: &Override,
        file_path: &Path,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Build include globset
        let mut include_builder = GlobSetBuilder::new();
        for pattern in &override_config.files {
            include_builder.add(Glob::new(pattern)?);
        }
        let include_set = include_builder.build()?;

        // Build exclude globset
        let mut exclude_builder = GlobSetBuilder::new();
        for pattern in &override_config.exclude {
            exclude_builder.add(Glob::new(pattern)?);
        }
        let exclude_set = exclude_builder.build()?;

        // Check if file matches include and doesn't match exclude
        let included = include_set.is_match(file_path);
        let excluded = exclude_set.is_match(file_path);

        Ok(included && !excluded)
    }

    /// Get a configuration section deserialized to type T
    ///
    /// Extra fields in the TOML are tolerated and ignored.
    /// If the section doesn't exist, returns the default value for T.
    ///
    /// # Example
    /// ```no_run
    /// use rustcop::config::{Config, ImportsConfig};
    /// use std::path::Path;
    ///
    /// let config = Config::load(Path::new("rustcop.toml")).unwrap();
    /// let imports_config = config.get_config::<ImportsConfig>("imports").unwrap();
    /// ```
    pub fn get_config<'de, T>(&self, section: &str) -> Result<T, Box<dyn std::error::Error>>
    where
        T: Deserialize<'de> + Default,
    {
        if let Some(section_value) = self.raw.get(section) {
            let config: T = section_value.clone().try_into()?;
            Ok(config)
        } else {
            Ok(T::default())
        }
    }

    /// Get the raw TOML value for advanced use cases
    pub fn raw(&self) -> &toml::Value {
        &self.raw
    }

    /// Get a nested configuration section (e.g., "lints.disallow_super_imports")
    ///
    /// # Example
    /// ```no_run
    /// use rustcop::config::{Config, LintConfig};
    /// use std::path::Path;
    ///
    /// let config = Config::load(Path::new("rustcop.toml")).unwrap();
    /// let lint = config.get_nested_config::<LintConfig>(&["lints", "disallow_super_imports"]).unwrap();
    /// ```
    pub fn get_nested_config<'de, T>(&self, path: &[&str]) -> Result<T, Box<dyn std::error::Error>>
    where
        T: Deserialize<'de> + Default,
    {
        let mut current = &self.raw;
        for segment in path {
            match current.get(segment) {
                Some(value) => current = value,
                None => return Ok(T::default()),
            }
        }
        let config: T = current.clone().try_into()?;
        Ok(config)
    }

    /// Get whether warnings should be treated as errors.
    /// Returns the configured value, defaulting to false if not set.
    pub fn treat_warnings_as_errors(&self) -> bool {
        self.raw
            .get("treat_warnings_as_errors")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Get whether suppression directives must include justifications.
    /// Returns the configured value, defaulting to true if not set.
    pub fn require_suppression_justification(&self) -> bool {
        self.raw
            .get("require_suppression_justification")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }
}

/// Configuration for the imports section
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct ImportsConfig {
    pub group_imports: bool,
    pub preferred_import_order: Vec<String>,
    pub import_granularity: String,
    pub import_merge_behaviour: String,
    pub allowed_import_prefixes: Vec<String>,
}

impl Default for ImportsConfig {
    fn default() -> Self {
        ImportsConfig {
            group_imports: true,
            preferred_import_order: vec![
                "std".to_string(),
                "extern_crate".to_string(),
                "crate".to_string(),
                "self".to_string(),
                "super".to_string(),
            ],
            import_granularity: "crate".to_string(),
            import_merge_behaviour: "always".to_string(),
            allowed_import_prefixes: vec![
                "std".to_string(),
                "extern_crate".to_string(),
                "crate".to_string(),
                "self".to_string(),
            ],
        }
    }
}

/// Configuration for the modules section
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct ModulesConfig {
    pub severity: String,
    pub preferred_module_order: Vec<String>,
    pub allowed_lib_exports: Vec<String>,
}

impl Default for ModulesConfig {
    fn default() -> Self {
        ModulesConfig {
            severity: "none".to_string(),
            preferred_module_order: vec![
                "local".to_string(),
                "crate".to_string(),
                "super".to_string(),
                "in_crate".to_string(),
            ],
            allowed_lib_exports: Vec::new(),
        }
    }
}

/// Configuration for a lint rule
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct LintConfig {
    pub severity: String,
}

impl Default for LintConfig {
    fn default() -> Self {
        LintConfig {
            severity: "none".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_with_extra_fields() {
        // TOML with extra fields that should be tolerated
        let toml_str = r#"
            [imports]
            group_imports = true
            import_granularity = "crate"
            some_future_field = "ignored"
            another_unknown_field = 42
        "#;

        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        let config = Config {
            raw,
            config_dir: None,
        };

        // Should successfully deserialize despite extra fields
        let imports = config.get_config::<ImportsConfig>("imports").unwrap();
        assert!(imports.group_imports);
        assert_eq!(imports.import_granularity, "crate");
    }

    #[test]
    fn test_get_config_missing_section() {
        let config = Config::empty();

        // Should return default when section doesn't exist
        let imports = config.get_config::<ImportsConfig>("imports").unwrap();
        assert_eq!(imports.import_granularity, "crate");
    }

    #[test]
    fn test_get_config_partial_fields() {
        // TOML with only some fields specified
        let toml_str = r#"
            [imports]
            group_imports = false
        "#;

        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        let config = Config {
            raw,
            config_dir: None,
        };

        let imports = config.get_config::<ImportsConfig>("imports").unwrap();
        assert!(!imports.group_imports);
        // Other fields should use defaults
        assert_eq!(imports.import_granularity, "crate");
    }

    #[test]
    fn test_get_nested_config() {
        let toml_str = r#"
            [lints.disallow_super_imports]
            severity = "error"
            
            [lints.another_rule]
            severity = "warning"
        "#;

        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        let config = Config {
            raw,
            config_dir: None,
        };

        let lint = config
            .get_nested_config::<LintConfig>(&["lints", "disallow_super_imports"])
            .unwrap();
        assert_eq!(lint.severity, "error");

        let lint2 = config
            .get_nested_config::<LintConfig>(&["lints", "another_rule"])
            .unwrap();
        assert_eq!(lint2.severity, "warning");

        // Non-existent lint should return default
        let lint3 = config
            .get_nested_config::<LintConfig>(&["lints", "nonexistent"])
            .unwrap();
        assert_eq!(lint3.severity, "none");
    }

    #[test]
    fn test_merge_tables() {
        let base_toml = r#"
            [imports]
            group_imports = true
            import_granularity = "crate"
            
            [lints.rule1]
            severity = "warning"
        "#;

        let override_toml = r#"
            [imports]
            group_imports = false
            
            [lints.rule1]
            message = "Custom"
            
            [lints.rule2]
            severity = "error"
        "#;

        let mut base: toml::Value = toml::from_str(base_toml).unwrap();
        let override_val: toml::Value = toml::from_str(override_toml).unwrap();

        Config::merge_tables(&mut base, &override_val);

        let config = Config {
            raw: base,
            config_dir: None,
        };

        // Scalar should be replaced
        let imports = config.get_config::<ImportsConfig>("imports").unwrap();
        assert!(!imports.group_imports);
        // But unspecified values should remain
        assert_eq!(imports.import_granularity, "crate");

        // Tables should be merged recursively
        let rule1 = config
            .get_nested_config::<LintConfig>(&["lints", "rule1"])
            .unwrap();
        assert_eq!(rule1.severity, "warning"); // Original value preserved

        let rule2 = config
            .get_nested_config::<LintConfig>(&["lints", "rule2"])
            .unwrap();
        assert_eq!(rule2.severity, "error"); // New value added
    }

    #[test]
    fn test_version_validation() {
        let valid_toml = r#"
            version = 1
            [imports]
            group_imports = true
        "#;

        let raw: toml::Value = toml::from_str(valid_toml).unwrap();
        assert!(Config::validate_version(&raw).is_ok());

        let invalid_toml = r#"
            version = 2
            [imports]
            group_imports = true
        "#;

        let raw: toml::Value = toml::from_str(invalid_toml).unwrap();
        assert!(Config::validate_version(&raw).is_err());

        // Missing version should produce warning but not error
        let no_version_toml = r#"
            [imports]
            group_imports = true
        "#;

        let raw: toml::Value = toml::from_str(no_version_toml).unwrap();
        assert!(Config::validate_version(&raw).is_ok());
    }

    #[test]
    fn test_override_matching() {
        use std::path::Path;

        let override_config = Override {
            files: vec!["tests/**/*.rs".to_string(), "**/*_test.rs".to_string()],
            exclude: vec!["tests/ignored/**".to_string()],
            config: toml::Value::Table(Default::default()),
        };

        // Should match files in tests/
        assert!(Config::matches_override(&override_config, Path::new("tests/foo.rs")).unwrap());

        assert!(
            Config::matches_override(&override_config, Path::new("tests/subdir/bar.rs")).unwrap()
        );

        // Should match *_test.rs anywhere
        assert!(Config::matches_override(&override_config, Path::new("src/my_test.rs")).unwrap());

        // Should not match excluded paths
        assert!(
            !Config::matches_override(&override_config, Path::new("tests/ignored/foo.rs")).unwrap()
        );

        // Should not match non-matching paths
        assert!(!Config::matches_override(&override_config, Path::new("src/main.rs")).unwrap());
    }
}
