//! Extracts Mozilla Fluent (`.ftl`) locale files from the Firefox source tree
//! so extracted components can use the same localization strings outside Firefox.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::errors::{Error, Result};

/// Maps `l10n-id` → `{ attr-name → English-value }` for static FTL entries.
pub type FtlMap = HashMap<String, HashMap<String, String>>;

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

/// Parses FTL files from the Firefox source tree into a flat map of
/// `l10n-id → { attr-name → English-value }` for static entries.
/// Entries whose values contain `{` (Fluent expressions) are skipped.
pub fn parse_ftl_map(ftl_references: &[String], firefox_root: &Path) -> FtlMap {
    let mut map = FtlMap::new();

    for ftl_ref in ftl_references {
        let source_path = resolve_ftl_source_path(firefox_root, ftl_ref);
        let content = match std::fs::read_to_string(&source_path) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("Warning: Could not read FTL file for parsing: {:?}", source_path);
                continue;
            }
        };

        let mut current_id: Option<String> = None;
        let mut current_attrs: HashMap<String, String> = HashMap::new();
        let mut current_value: Option<String> = None;

        for line in content.lines() {
            // Comment or blank line
            if line.starts_with('#') || line.trim().is_empty() {
                // Flush current entry
                if let Some(id) = current_id.take() {
                    flush_entry(&mut map, id, current_value.take(), current_attrs.drain().collect());
                    current_attrs.clear();
                }
                continue;
            }

            // New message identifier (no leading whitespace)
            if !line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with('.') {
                // Flush previous entry
                if let Some(id) = current_id.take() {
                    flush_entry(&mut map, id, current_value.take(), current_attrs.drain().collect());
                    current_attrs.clear();
                }

                if let Some((id, rest)) = line.split_once('=') {
                    let id = id.trim().to_string();
                    let value = rest.trim();
                    if value.is_empty() {
                        current_id = Some(id);
                        current_value = None;
                    } else if !value.contains('{') {
                        current_id = Some(id);
                        current_value = Some(value.to_string());
                    } else {
                        // Dynamic value — still parse attributes
                        current_id = Some(id);
                        current_value = None;
                    }
                }
                continue;
            }

            // Attribute line (indented, starts with `.`)
            let trimmed = line.trim();
            if trimmed.starts_with('.') {
                if let Some(attr_content) = trimmed.strip_prefix('.') {
                    if let Some((attr_name, attr_value)) = attr_content.split_once('=') {
                        let attr_name = attr_name.trim().to_string();
                        let attr_value = attr_value.trim().to_string();
                        if !attr_value.contains('{') {
                            current_attrs.insert(attr_name, attr_value);
                        }
                    }
                }
            }
        }

        // Flush last entry
        if let Some(id) = current_id.take() {
            flush_entry(&mut map, id, current_value.take(), current_attrs);
        }
    }

    map
}

fn flush_entry(
    map: &mut FtlMap,
    id: String,
    value: Option<String>,
    attrs: HashMap<String, String>,
) {
    if value.is_none() && attrs.is_empty() {
        return;
    }
    let mut entry = HashMap::new();
    if let Some(v) = value {
        entry.insert(".value".to_string(), v);
    }
    entry.extend(attrs);
    map.insert(id, entry);
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
