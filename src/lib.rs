//! Top-level pipeline orchestrator. Wires together JAR resolution, dependency graphing,
//! file transformation, and code generation into a single `transform_lib` entry point.

use std::collections::HashMap;
use std::path::Path;

mod dependencies;
mod dependency_graph;
pub mod errors;
mod pipeline;
mod transform;
mod utils;

use dependency_graph::DependencyGraph;
use errors::{Error, Result};
use utils::{file_utils, jar_resolver};

/// Runs the full extraction pipeline: resolves chrome:// URLs, discovers components,
/// builds the dependency graph, transforms all files, and writes output to `output_path`.
pub fn transform_lib(
    firefox_root: &Path,
    output_path: &str,
    jar_paths: &[String],
    mozbuild_paths: &[String],
    global_stylesheets: &[String],
    component_paths: &[String],
    docs_paths: &[String],
    fluent_fallbacks: &HashMap<String, String>,
) -> Result<()> {
    // Parse JAR mappings for chrome:// URL resolution
    let jr = jar_resolver::JarResolver::new(firefox_root, jar_paths, mozbuild_paths, None)
        .map_err(|e| Error::Custom(format!("Failed to parse JAR mappings: {e}")))?;

    let pf = utils::path_finder::PathFinder::new(jr);

    let output_dir = Path::new(output_path);

    file_utils::ensure_directory_exists(output_dir)
        .map_err(|e| Error::Custom(format!("Failed to ensure directory exists: {e}")))?;
    file_utils::clear_directory(output_dir)
        .map_err(|e| Error::Custom(format!("Failed to clear directory: {e}")))?;

    // Create output directories
    file_utils::create_output_directories(output_dir)
        .map_err(|e| Error::Custom(format!("Failed to create output directories: {e}")))?;

    // Initialize dependency graph
    let mut dep_graph = DependencyGraph::new();

    // Process components first
    eprintln!("Processing components...");
    pipeline::discovery::process_components(firefox_root, component_paths, &mut dep_graph)?;

    // Process global stylesheets
    eprintln!("Processing global stylesheets...");
    pipeline::discovery::process_global_stylesheets(
        firefox_root,
        global_stylesheets,
        &mut dep_graph,
    )?;

    // Process all dependencies recursively
    eprintln!("Processing dependencies...");
    let ftl_references = pipeline::dependency_walker::process_dependencies(&mut dep_graph, &pf)?;
    #[cfg(debug_assertions)]
    dep_graph.debug_print();

    // Extract FTL locale files
    eprintln!("Extracting FTL locale files...");
    let ftl_files: Vec<String> = ftl_references.into_iter().collect();
    pipeline::fluent::extract_ftl_files(firefox_root, output_dir, &ftl_files)?;

    // Parse FTL files into a map for injecting English fallback attributes
    eprintln!("Parsing FTL files for English fallbacks...");
    let ftl_map = pipeline::fluent::parse_ftl_map(&ftl_files, firefox_root);

    // Generate the optional fluent-setup.mjs for advanced locale customization
    eprintln!("Generating fluent-setup module...");
    pipeline::codegen::generate_fluent_setup(output_dir, &ftl_files)?;

    // Generate acorn-icon component for cross-browser colorable SVG icons
    eprintln!("Generating acorn-icon component...");
    pipeline::codegen::generate_acorn_icon_component(output_dir)?;

    // Transform and write all files
    eprintln!("Transforming and writing files...");
    pipeline::writer::transform_and_write_files(&mut dep_graph, output_dir, &ftl_map, fluent_fallbacks)?;

    // Process and write documentation files (.stories.md -> .stories.mdx)
    eprintln!("Processing documentation files...");
    pipeline::docs::process_and_write_docs(firefox_root, output_dir, docs_paths, &dep_graph)?;

    // Generate index files for library consumption
    eprintln!("Generating index files...");
    pipeline::codegen::generate_index_files(&dep_graph, output_dir)?;

    Ok(())
}
