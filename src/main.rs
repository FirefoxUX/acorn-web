//! CLI entry point. Parses arguments, loads config, and invokes the transformation pipeline.

mod config;

use std::fs;
use std::path::Path;

use clap::Parser;
use thiserror::Error;

use config::Config;
use mozcomp::transform_lib;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the Firefox root directory
    firefox_root: String,

    /// Path to the output directory
    #[arg(default_value = "output")]
    output: String,

    /// Path to the configuration file
    #[arg(default_value = "mozcomp.toml")]
    config: String,

    /// Path to chrome-map.json (overrides config file; relative to firefox_root)
    #[arg(long)]
    chrome_map: Option<String>,
}

#[derive(Error, Debug)]
pub enum MainError {
    #[error("Failed to read config file: {0}")]
    ConfigReadError(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    ConfigParseError(#[from] toml::de::Error),
    #[error("Failed to transform library: {0}")]
    TransformError(#[from] mozcomp::errors::Error),
    #[error("Could not find chrome-map.json in {0}\n  Generate it with: cd <firefox_root> && ./mach build-backend --backend ChromeMap")]
    ChromeMapNotFound(String),
}

/// Auto-detect chrome-map.json by finding `obj-*/chrome-map.json` in the Firefox root.
fn find_chrome_map(firefox_root: &Path) -> Result<std::path::PathBuf, MainError> {
    if let Ok(entries) = fs::read_dir(firefox_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("obj-") && entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                let candidate = entry.path().join("chrome-map.json");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(MainError::ChromeMapNotFound(
        firefox_root.display().to_string(),
    ))
}

fn main() -> Result<(), MainError> {
    let args = Args::parse();
    let firefox_root = Path::new(&args.firefox_root);

    // Read and parse the config file
    let config_str = fs::read_to_string(&args.config)?;
    let config: Config = toml::from_str(&config_str)?;

    // Resolve chrome-map.json path: CLI flag > config file > auto-detect
    let chrome_map_path = if let Some(ref cli_path) = args.chrome_map {
        firefox_root.join(cli_path)
    } else if let Some(ref cfg_path) = config.chrome_map_path {
        firefox_root.join(cfg_path)
    } else {
        find_chrome_map(firefox_root)?
    };

    // Call the transform_lib function with the parsed configuration
    transform_lib(
        firefox_root,
        &args.output,
        &chrome_map_path,
        &config.globals_stylesheets,
        &config.component_paths,
        &config.docs_paths,
    )?;
    Ok(())
}
