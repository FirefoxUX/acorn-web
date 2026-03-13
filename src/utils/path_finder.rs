//! Resolves import strings (`chrome://`, `resource://`, relative paths) to absolute
//! filesystem paths using the chrome-map resolver for internal URLs.

use crate::utils::chrome_map_resolver::ChromeMapResolver;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathFinderError {
    #[error("Chrome mapping not found for URL: {0}")]
    ChromeMappingNotFound(String),
    #[error("Could not resolve relative path from '{from}' to '{import}'")]
    RelativePathResolutionFailed { from: PathBuf, import: String },
    #[error("Import string is empty")]
    EmptyImportString,
    #[error("Unsupported import format: {0}")]
    UnsupportedImportFormat(String),
    #[error("File does not exist: {0}")]
    FileNotFound(PathBuf),
}

/// Resolves import strings to absolute filesystem paths. Handles `chrome://` and
/// `resource://` via the chrome-map resolver, and relative paths via the importing file's location.
pub struct PathFinder {
    resolver: ChromeMapResolver,
}

impl PathFinder {
    /// Create a new PathFinder with a ChromeMapResolver
    pub fn new(resolver: ChromeMapResolver) -> Self {
        Self { resolver }
    }

    /// Resolve an import string to a PathBuf relative to the current working directory
    ///
    /// # Arguments
    /// * `current_file` - The file that contains the import statement
    /// * `import_string` - The import string to resolve (e.g., "./utils.js", "chrome://resources/...")
    ///
    /// # Returns
    /// The resolved PathBuf
    pub fn get_path(
        &self,
        current_file: &Path,
        import_string: &str,
    ) -> Result<PathBuf, PathFinderError> {
        let import_string = import_string.trim();

        if import_string.is_empty() {
            return Err(PathFinderError::EmptyImportString);
        }

        let resolved_path = if self.resolver.is_internal_url(import_string) {
            self.resolver
                .resolve_url(import_string)
                .map_err(|e| match e {
                    crate::utils::chrome_map_resolver::ChromeMapError::InvalidUrl(url) => {
                        PathFinderError::UnsupportedImportFormat(url)
                    }
                    crate::utils::chrome_map_resolver::ChromeMapError::NoMappingFound(url) => {
                        PathFinderError::ChromeMappingNotFound(url)
                    }
                    _ => PathFinderError::UnsupportedImportFormat(import_string.to_string()),
                })?
        } else if self.is_relative_path(import_string) {
            self.resolve_relative_path(current_file, import_string)?
        } else {
            return Err(PathFinderError::UnsupportedImportFormat(
                import_string.to_string(),
            ));
        };

        // Convert to relative path from current working directory using file_utils
        let rel_source_path = super::file_utils::make_relative_to_cwd(&resolved_path);

        // Verify file exists if enabled
        if !resolved_path.exists() {
            return Err(PathFinderError::FileNotFound(resolved_path));
        }

        Ok(rel_source_path)
    }

    /// Check if an import string represents a relative path
    fn is_relative_path(&self, import_string: &str) -> bool {
        import_string.starts_with("./") || 
        import_string.starts_with("../") || 
        import_string.starts_with('/') ||
        // Handle paths without explicit relative indicators but with file extensions
        (!import_string.contains("://") && 
         (import_string.ends_with(".js") || 
          import_string.ends_with(".css") || 
          import_string.ends_with(".ts") ||
          import_string.ends_with(".jsx") ||
          import_string.ends_with(".tsx") ||
          import_string.contains('/')))
    }

    /// Resolve a relative path based on the current file location
    fn resolve_relative_path(
        &self,
        current_file: &Path,
        import_string: &str,
    ) -> Result<PathBuf, PathFinderError> {
        let current_dir =
            current_file
                .parent()
                .ok_or_else(|| PathFinderError::RelativePathResolutionFailed {
                    from: current_file.to_path_buf(),
                    import: import_string.to_string(),
                })?;

        // Handle different relative path formats
        let resolved = if import_string.starts_with('/') {
            // Absolute path from root
            PathBuf::from(import_string)
        } else {
            // Relative path from current directory
            current_dir.join(import_string)
        };

        // Canonicalize to resolve .. and . components
        let canonical = resolved.canonicalize().unwrap_or_else(|_| {
            // If canonicalize fails, try manual resolution
            super::file_utils::normalize_path(&resolved)
        });

        Ok(canonical)
    }
}
