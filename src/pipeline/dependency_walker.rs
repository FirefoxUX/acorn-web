//! Recursively resolves all transitive dependencies of discovered files, building the
//! complete dependency graph. Also collects FTL (Fluent localization) references.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::dependency_graph::{DependencyGraph, FileType, TargetLocation};
use crate::errors::{Error, Result};
use crate::utils::path_finder::PathFinder;
use crate::{dependencies, transform, utils};

/// Recursively resolves all transitive dependencies starting from files already in the graph.
/// Uses a worklist algorithm: pops a file, extracts its imports (JS via `oxc`, CSS via
/// `lightningcss`), resolves each to a filesystem path, adds new nodes/edges to the graph,
/// and pushes unprocessed files back onto the worklist. Returns all FTL references found.
pub fn process_dependencies(
    dep_graph: &mut DependencyGraph,
    path_finder: &PathFinder,
) -> Result<HashSet<String>> {
    let mut processed: HashSet<PathBuf> = HashSet::new();
    let mut to_process: Vec<crate::dependency_graph::FileNode> =
        dep_graph.all_files().cloned().collect();
    let mut ftl_references: HashSet<String> = HashSet::new();

    // --- Worklist loop: pop a file, extract its imports, resolve each, add to graph ---
    while let Some(file) = to_process.pop() {
        if !processed.insert(file.path.clone()) {
            continue;
        }

        // Extract imports: JS files via oxc AST visitor, CSS files via lightningcss visitor
        let deps = match file.file_type {
            FileType::JsComponent | FileType::JsFile => {
                let js_deps =
                    dependencies::js::dependencies_from_file(&file.path).map_err(|e| {
                        Error::Custom(format!(
                            "Failed to parse JS dependencies for {:?}: {}",
                            file.path, e
                        ))
                    })?;
                ftl_references.extend(js_deps.ftl_references);
                js_deps.imports
            }
            FileType::CssFile => {
                dependencies::css::dependencies_from_file(&file.path).map_err(|e| {
                    Error::Custom(format!(
                        "Failed to parse CSS dependencies for {:?}: {}",
                        file.path, e
                    ))
                })?
            }
            _ => vec![],
        };

        // --- Resolve each dependency and classify by file type / target location ---
        for dep in deps {
            if utils::url::is_unresolvable_firefox_url(&dep) {
                continue;
            }

            // Resolve the dependency path
            let resolved_path = match path_finder.get_path(&file.path, &dep) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!(
                        "Failed to resolve path for dependency '{}': {:?}",
                        &file.path.display(),
                        e
                    );
                    continue;
                }
            };

            // Classify the dependency: file type determines how it will be parsed/transformed,
            // target location determines where it lands in dist/ (or if it's omitted/inlined)
            let dep_file_type = match Path::new(&dep).extension().and_then(|s| s.to_str()) {
                Some("css") => FileType::CssFile,
                Some("js") | Some("mjs") => FileType::JsFile,
                _ => FileType::OpaqueFile,
            };

            let dep_target_location = match (
                &file.file_type,
                Path::new(&dep).extension().and_then(|s| s.to_str()),
            ) {
                (FileType::JsComponent, Some("css")) => TargetLocation::Omit,
                (_, Some("png") | Some("jpg") | Some("jpeg") | Some("svg")) => {
                    TargetLocation::Asset
                }
                _ => TargetLocation::Dependency,
            };

            // Add file to dependency graph; if it is new, push to to_process
            dep_graph.add_file(resolved_path.clone(), dep_file_type, dep_target_location);
            dep_graph
                .add_dependency(&file.path, &resolved_path, &dep)
                .map_err(|e| Error::Custom(format!("Failed to add dependency: {e}")))?;

            // CSS imported by a component is marked Omit (it gets inlined). But if a
            // non-component file also imports that CSS, promote it to Dependency so it's emitted.
            if file.file_type != FileType::JsComponent {
                dep_graph.promote_target_location(&resolved_path, TargetLocation::Dependency);
            }

            // Only process if not already processed and not already queued
            if !processed.contains(&resolved_path)
                && !to_process.iter().any(|f| f.path == resolved_path)
                && let Some(node) = dep_graph.get_file(&resolved_path)
            {
                to_process.push(node.clone());
            }
        }
    }

    Ok(ftl_references)
}

/// For a JS component that imports CSS files (marked as `Omit` since they'll be inlined),
/// transforms each CSS file and returns a map of `original_import -> transformed_css_code`.
/// Returns `None` if the file has no CSS imports.
pub fn build_css_replacements(
    dep_graph: &DependencyGraph,
    file: &crate::dependency_graph::FileNode,
) -> Result<Option<HashMap<String, String>>> {
    let omitted_imports = dep_graph.get_css_imports(&file.path);
    if omitted_imports.is_empty() {
        return Ok(None);
    }

    let mut css_replacements = HashMap::new();
    for (original_path, css_path) in omitted_imports {
        let relative_imports = dep_graph
            .get_dependencies_and_relative_paths(&css_path, &file.path)
            .map_err(|e| {
                Error::Custom(format!(
                    "Failed to get CSS dependency paths for {:?}: {e}",
                    css_path
                ))
            })?;
        let css_code =
            transform::css::transform_from_file(&css_path, &relative_imports).map_err(|e| {
                Error::Custom(format!(
                    "Failed to transform CSS file: {:?}: {}",
                    css_path, e
                ))
            })?;
        css_replacements.insert(original_path, css_code);
    }
    Ok(Some(css_replacements))
}
