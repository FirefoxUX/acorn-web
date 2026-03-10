//! Processes standalone documentation files (`.stories.md`) into Storybook-compatible
//! `.mdx` format for the docs section of the component library.

use std::path::{Path, PathBuf};

use glob::glob;

use crate::dependency_graph::DependencyGraph;
use crate::errors::{Error, Result};
use crate::transform;

/// Processes standalone documentation files (`.stories.md`) from `docs_paths` glob patterns
/// and writes them as Storybook `.mdx` files to `{output_dir}/docs/`.
pub fn process_and_write_docs(
    firefox_root: &Path,
    output_dir: &Path,
    docs_paths: &[String],
    dep_graph: &DependencyGraph,
) -> Result<()> {
    let global_chrome_map = dep_graph.build_global_chrome_url_map();

    let docs_dir = output_dir.join("docs");
    std::fs::create_dir_all(&docs_dir).map_err(|e| {
        Error::Custom(format!(
            "Failed to create docs directory {:?}: {e}",
            docs_dir
        ))
    })?;

    let mut count = 0;
    for pattern in docs_paths {
        let full_pattern = firefox_root.join(pattern.trim_start_matches('/'));
        let full_pattern_str = full_pattern.to_string_lossy();

        let files: Vec<PathBuf> = glob(&full_pattern_str)?
            .filter_map(|r| r.ok())
            .collect();

        for file_path in files {
            let file_name = match file_path.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            let title = transform::markdown::title_for_global_doc(&file_name);

            let content = std::fs::read_to_string(&file_path).map_err(|e| {
                Error::Custom(format!(
                    "Failed to read doc file {:?}: {e}",
                    file_path
                ))
            })?;

            let mdx =
                transform::markdown::transform_stories_md(&content, &title, &global_chrome_map);

            let mdx_name = file_name.replace(".stories.md", ".mdx");
            let output_path = docs_dir.join(&mdx_name);

            std::fs::write(&output_path, mdx).map_err(|e| {
                Error::Custom(format!(
                    "Failed to write doc MDX file {:?}: {e}",
                    output_path
                ))
            })?;

            eprintln!("  Doc: {} -> {}", file_name, mdx_name);
            count += 1;
        }
    }

    eprintln!("Processed {} documentation files", count);
    Ok(())
}
