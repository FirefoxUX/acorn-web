//! Configuration file deserialization. Loaded from `config.toml` at the CLI.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub globals_stylesheets: Vec<String>,
    pub component_paths: Vec<String>,
    #[serde(default)]
    pub docs_paths: Vec<String>,
    /// Path to chrome-map.json, relative to firefox_root. Auto-detected if not set.
    pub chrome_map_path: Option<String>,
}
