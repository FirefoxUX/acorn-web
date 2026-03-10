//! CLI entry point. Parses arguments, loads config, and invokes the transformation pipeline.

mod config;

use std::fs;

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
}

#[derive(Error, Debug)]
pub enum MainError {
    #[error("Failed to read config file: {0}")]
    ConfigReadError(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    ConfigParseError(#[from] toml::de::Error),
    #[error("Failed to transform library: {0}")]
    TransformError(#[from] mozcomp::errors::Error),
}

fn main() -> Result<(), MainError> {
    let args = Args::parse();

    // Read and parse the config file
    let config_str = fs::read_to_string(&args.config)?;
    let config: Config = toml::from_str(&config_str)?;

    // Call the transform_lib function with the parsed configuration
    transform_lib(
        std::path::Path::new(&args.firefox_root),
        &args.output,
        &config.jar_paths,
        &config.mozbuild_paths,
        &config.globals_stylesheets,
        &config.component_paths,
        &config.docs_paths,
        &config.fluent_fallbacks,
    )?;
    Ok(())
}
