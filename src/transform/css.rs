//! CSS transformation pipeline. Parses with `lightningcss`, rewrites `@import` and `url()`
//! references via visitor pattern, then applies string-based icon property transforms.

use lightningcss::{
    printer::PrinterOptions,
    stylesheet::{ParserFlags, ParserOptions, StyleSheet},
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::{
    errors::{Error, Result},
    transform::css_transform::{ImportReplacer, UrlReplacer, icon_property_transform},
    utils::url::replace_chrome_urls,
};

/// Reads a CSS file and applies all transformations (URL replacement, import rewriting,
/// icon property conversion, remaining chrome:// URL cleanup).
pub fn transform_from_file(
    source_path: &Path,
    url_replacements: &HashMap<String, String>,
) -> Result<String> {
    let css_content = fs::read_to_string(source_path)?;
    transform_from_string(&css_content, url_replacements)
}

/// Transforms CSS source through two phases:
/// 1. **AST phase** (`lightningcss`): parse -> visit urls and @imports to rewrite paths
/// 2. **String phase**: icon property transform + catch any chrome:// URLs the visitor missed
pub fn transform_from_string(
    css_content: &str,
    url_replacements: &HashMap<String, String>,
) -> Result<String> {
    // Parse the CSS using StyleSheet::parse

    let mut stylesheet = StyleSheet::parse(
        css_content,
        ParserOptions {
            flags: ParserFlags::NESTING,
            ..Default::default()
        },
    )
    .map_err(|e| Error::CssParse {
        message: format!("{:?}", e),
    })?;

    // Use UrlReplacer to mutate the stylesheet in place
    UrlReplacer::new(url_replacements).build(&mut stylesheet)?;
    ImportReplacer::new(url_replacements).build(&mut stylesheet)?;

    // Serialize the transformed stylesheet back to CSS
    let result = stylesheet
        .to_css(PrinterOptions::default())
        .map_err(|e| Error::CssSerialize {
            message: format!("{:?}", e),
        })?;

    // Post-process: transform -moz-context-properties icon patterns to mask-image
    let code = icon_property_transform::transform_icon_properties(&result.code);

    // Replace any remaining chrome:// URLs not caught by the lightningcss URL visitor
    // (e.g., bare strings in image-set() which lightningcss doesn't visit as URLs)
    let code = replace_chrome_urls(&code, url_replacements);

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_url_replacement() {
        let mut replacements = HashMap::new();
        replacements.insert(
            "chrome://global/skin/design-tokens.css".to_string(),
            "../styles/design-tokens.css".to_string(),
        );
        let css = r#"@import url("chrome://global/skin/design-tokens.css");"#;
        let result = transform_from_string(css, &replacements).unwrap();
        assert!(result.contains("../styles/design-tokens.css"));
        assert!(!result.contains("chrome://"));
    }

    #[test]
    fn test_url_property_replacement() {
        let mut replacements = HashMap::new();
        replacements.insert(
            "chrome://global/skin/icon.svg".to_string(),
            "../assets/icon.svg".to_string(),
        );
        let css = r#".icon { background-image: url("chrome://global/skin/icon.svg"); }"#;
        let result = transform_from_string(css, &replacements).unwrap();
        assert!(result.contains("../assets/icon.svg"));
    }

    #[test]
    fn test_passthrough_no_replacements() {
        let css = ".foo { color: red; }";
        let result = transform_from_string(css, &HashMap::new()).unwrap();
        assert!(result.contains("color: red"));
    }
}
