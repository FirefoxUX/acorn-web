//! Transforms and writes all files from the dependency graph to the output directory.
//! Handles JS, CSS, SVG, markdown, and opaque files with type-specific transformations.

use std::collections::HashMap;
use std::path::Path;

use crate::dependency_graph::{DependencyGraph, FileType, TargetLocation};
use crate::errors::{Error, Result};
use crate::pipeline::dependency_walker::build_css_replacements;
use crate::pipeline::fluent::FtlMap;
use crate::pipeline::svg::transform_svg_context_fill;
use crate::utils::file_utils;
use crate::transform;

/// Iterates all non-omitted files in the dependency graph, applies type-specific
/// transformations (JS import rewriting, CSS URL replacement, SVG context-fill, markdown
/// MDX conversion), and writes each to its output path under `output_dir`.
pub fn transform_and_write_files(dep_graph: &mut DependencyGraph, output_dir: &Path, ftl_map: &FtlMap, fluent_fallbacks: &HashMap<String, String>) -> Result<()> {
    // Build a global chrome:// URL -> dist path map for replacing URLs that aren't
    // direct dependencies of the file being transformed (e.g., chrome:// URLs in
    // story templates that reference assets from other components).
    let global_chrome_map = dep_graph.build_global_chrome_url_map();

    let files = dep_graph.all_files().filter(|f| {
        f.target_location != TargetLocation::Omit
    });

    for file in files {
        let output_path = match file.get_dist_path() {
            Some(path) => output_dir.join(path),
            None => {
                continue;
            }
        };

        // Ensure the parent directory exists before writing/copying
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::Custom(format!("Failed to create directory: {:?}: {e}", parent))
            })?;
        }

        match file.file_type {
            FileType::JsComponent | FileType::JsFile => {
                let mut relative_imports =
                    dep_graph.get_import_replacements(&file.path).map_err(|e| {
                        Error::Custom(format!(
                            "Failed to get import replacements for {:?}: {e}",
                            file.path
                        ))
                    })?;

                // Merge global chrome:// URL mappings (for URLs not in this file's
                // direct dependencies, e.g., chrome:// in story template attributes)
                if let Some(current_dist) = file.get_dist_path() {
                    for (chrome_url, target_dist) in &global_chrome_map {
                        if !relative_imports.contains_key(chrome_url) {
                            let rel =
                                file_utils::compute_relative_path(&current_dist, target_dist);
                            relative_imports.insert(chrome_url.clone(), rel);
                        }
                    }
                }

                let css_replacements = build_css_replacements(dep_graph, file)?;

                let transformed_code = transform::js::transform_from_file(
                    &file.path,
                    &relative_imports,
                    css_replacements.as_ref(),
                    ftl_map,
                    fluent_fallbacks,
                )
                .map_err(|e| {
                    Error::Custom(format!(
                        "Failed to transform JS file: {:?}: {}",
                        file.path, e
                    ))
                })?;
                std::fs::write(&output_path, transformed_code).map_err(|e| {
                    Error::Custom(format!("Failed to write JS file: {:?}: {e}", file.path))
                })?;
            }
            FileType::CssFile => {
                let mut relative_imports =
                    dep_graph.get_import_replacements(&file.path).map_err(|e| {
                        Error::Custom(format!(
                            "Failed to get import replacements for {:?}: {e}",
                            file.path
                        ))
                    })?;

                // Merge global chrome:// URL mappings for CSS files too
                if let Some(current_dist) = file.get_dist_path() {
                    for (chrome_url, target_dist) in &global_chrome_map {
                        if !relative_imports.contains_key(chrome_url) {
                            let rel =
                                file_utils::compute_relative_path(&current_dist, target_dist);
                            relative_imports.insert(chrome_url.clone(), rel);
                        }
                    }
                }

                let transformed_code =
                    transform::css::transform_from_file(&file.path, &relative_imports).map_err(
                        |e| {
                            Error::Custom(format!(
                                "Failed to transform CSS file: {:?}: {e}",
                                file.path
                            ))
                        },
                    )?;
                std::fs::write(&output_path, transformed_code).map_err(|e| {
                    Error::Custom(format!("Failed to write CSS file: {:?}: {e}", file.path))
                })?;
            }
            _ => {
                let ext = file.path.extension().and_then(|s| s.to_str());
                let file_name = file.path.file_name().and_then(|s| s.to_str()).unwrap_or("");

                if ext == Some("svg") {
                    // SVG files need context-fill rewriting for cross-browser compatibility
                    let svg_content = std::fs::read_to_string(&file.path).map_err(|e| {
                        Error::Custom(format!("Failed to read SVG file: {:?}: {e}", file.path))
                    })?;
                    let transformed = transform_svg_context_fill(&svg_content);
                    std::fs::write(&output_path, transformed).map_err(|e| {
                        Error::Custom(format!("Failed to write SVG file: {:?}: {e}", file.path))
                    })?;
                } else if file_name.ends_with(".stories.md") {
                    // Convert .stories.md -> .stories.mdx for Storybook rendering
                    let component_folder = file
                        .path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown");
                    let title =
                        transform::markdown::title_for_component_readme(component_folder);
                    let content = std::fs::read_to_string(&file.path).map_err(|e| {
                        Error::Custom(format!(
                            "Failed to read stories.md file: {:?}: {e}",
                            file.path
                        ))
                    })?;
                    let mdx =
                        transform::markdown::transform_stories_md(&content, &title, &global_chrome_map);
                    // Write as .mdx (Storybook 7+ uses .mdx for docs, not .stories.mdx)
                    let mdx_path = output_path
                        .to_string_lossy()
                        .replace(".stories.md", ".mdx");
                    std::fs::write(&mdx_path, mdx).map_err(|e| {
                        Error::Custom(format!(
                            "Failed to write MDX file: {:?}: {e}",
                            file.path
                        ))
                    })?;
                } else {
                    // other files are copied as is
                    std::fs::copy(&file.path, &output_path).map_err(|e| {
                        Error::Custom(format!("Failed to copy file: {:?}: {e}", file.path))
                    })?;
                }
            }
        }
    }

    Ok(())
}
