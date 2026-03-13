//! Discovers component files and global stylesheets by globbing the Firefox source tree
//! using patterns from `config.toml`.

use std::path::{Path, PathBuf};

use glob::glob;

use crate::dependency_graph::{DependencyGraph, FileType, TargetLocation};
use crate::errors::{Error, Result};
use crate::utils::file_utils;

/// Globs `component_paths` patterns against `firefox_root` and adds each matched file
/// to the dependency graph. Classifies files as `JsComponent`, `JsFile`, or `OpaqueFile`
/// based on extension and filename patterns.
pub fn process_components(
    firefox_root: &Path,
    component_paths: &[String],
    dep_graph: &mut DependencyGraph,
) -> Result<()> {
    for pattern in component_paths {
        let full_pattern = firefox_root.join(pattern.trim_start_matches('/'));
        let full_pattern_str = full_pattern.to_string_lossy();

        let files: Vec<PathBuf> = glob(&full_pattern_str)?.filter_map(|r| r.ok()).collect();

        for file_path in files {
            let file_path = file_utils::make_relative_to_cwd(&file_path);
            let file_name = file_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            // Ignore .ts, .tsx, .css files
            if file_name.ends_with(".ts") || file_name.ends_with(".css") {
                continue;
            }

            let file_type =
                if file_name.ends_with(".stories.mjs") || file_name.ends_with(".story.mjs") {
                    FileType::JsFile
                } else if file_name.ends_with(".mjs") {
                    FileType::JsComponent
                } else {
                    FileType::OpaqueFile
                };

            // Get the name of the folder the file is directly in
            let component_name = file_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .ok_or_else(|| {
                    Error::Custom(format!(
                        "Failed to determine component folder for {:?}",
                        file_path
                    ))
                })?;

            dep_graph.add_file(
                file_path.clone(),
                file_type,
                TargetLocation::Component(component_name.to_string()),
            );
        }
    }
    Ok(())
}

/// Globs `stylesheet_paths` patterns and adds matched CSS files to the dependency graph
/// with `CssGlobal` target location (output to `dist/styles/`).
pub fn process_global_stylesheets(
    firefox_root: &Path,
    stylesheet_paths: &[String],
    dep_graph: &mut DependencyGraph,
) -> Result<()> {
    for pattern in stylesheet_paths {
        let full_pattern = firefox_root.join(pattern.trim_start_matches('/'));
        let full_pattern_str = full_pattern.to_string_lossy();

        let files: Vec<PathBuf> = glob(&full_pattern_str)?.filter_map(|r| r.ok()).collect();

        for file_path in files {
            let file_path = file_utils::make_relative_to_cwd(&file_path);
            dep_graph.add_file(
                file_path.clone(),
                FileType::CssFile,
                TargetLocation::CssGlobal,
            );
        }
    }
    Ok(())
}
