//! Configuration file deserialization. Loaded from `config.toml` at the CLI.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub globals_stylesheets: Vec<String>,
    pub jar_paths: Vec<String>,
    pub mozbuild_paths: Vec<String>,
    pub component_paths: Vec<String>,
    #[serde(default)]
    pub docs_paths: Vec<String>,
    #[serde(default)]
    pub fluent_fallbacks: HashMap<String, String>,
}
