//! Extracts Mozilla Fluent (`.ftl`) locale files from the Firefox source tree
//! so extracted components can use the same localization strings outside Firefox.

use std::path::{Path, PathBuf};

use crate::errors::{Error, Result};

/// Maps an FTL reference path to the corresponding file in the Firefox source tree.
/// E.g., `toolkit/global/mozButton.ftl` -> `{firefox_root}/toolkit/locales/en-US/toolkit/global/mozButton.ftl`
pub fn resolve_ftl_source_path(firefox_root: &Path, ftl_ref: &str) -> PathBuf {
    let (root, _) = ftl_ref.split_once('/').unwrap_or((ftl_ref, ""));
    match root {
        "toolkit" => firefox_root.join("toolkit/locales/en-US").join(ftl_ref),
        "browser" => firefox_root.join("browser/locales/en-US").join(ftl_ref),
        "branding" => {
            let rest = ftl_ref.strip_prefix("branding/").unwrap_or(ftl_ref);
            firefox_root
                .join("browser/branding/nightly/locales/en-US")
                .join(rest)
        }
        _ => firefox_root.join("toolkit/locales/en-US").join(ftl_ref),
    }
}

/// Copies referenced FTL files from the Firefox source tree to `{output_dir}/locales/en-US/`.
pub fn extract_ftl_files(
    firefox_root: &Path,
    output_dir: &Path,
    ftl_files: &[String],
) -> Result<()> {
    if ftl_files.is_empty() {
        return Ok(());
    }

    let locale_dir = output_dir.join("locales/en-US");

    for ftl_ref in ftl_files {
        let source_path = resolve_ftl_source_path(firefox_root, ftl_ref);
        let dest_path = locale_dir.join(ftl_ref);

        if !source_path.exists() {
            eprintln!("Warning: FTL file not found: {:?}", source_path);
            continue;
        }

        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::Custom(format!("Failed to create FTL directory {:?}: {e}", parent))
            })?;
        }

        std::fs::copy(&source_path, &dest_path).map_err(|e| {
            Error::Custom(format!("Failed to copy FTL file {:?}: {e}", source_path))
        })?;
        eprintln!("  Extracted: {}", ftl_ref);
    }

    eprintln!("Extracted {} FTL files", ftl_files.len());
    Ok(())
}
