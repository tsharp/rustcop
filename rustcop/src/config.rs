use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub imports: ImportsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ImportsConfig {
    pub enabled: bool,
    pub group: bool,
    pub sort: bool,
    pub merge: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            imports: ImportsConfig::default(),
        }
    }
}

impl Default for ImportsConfig {
    fn default() -> Self {
        ImportsConfig {
            enabled: true,
            group: true,
            sort: true,
            merge: true,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
