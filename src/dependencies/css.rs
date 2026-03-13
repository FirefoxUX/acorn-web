//! Extracts import dependencies from CSS files using the `lightningcss` visitor API.
//! Collects both `@import` URLs and `url()` references.

use lightningcss::{
    rules::CssRule,
    stylesheet::{ParserFlags, ParserOptions, StyleSheet},
    values::url::Url,
    visitor::{Visit, VisitTypes, Visitor},
};
use std::fs;
use std::path::Path;

use crate::errors::{Error, Result};
use crate::utils::url::is_external_url;

/// Reads a CSS file and extracts its import dependencies (`@import` and `url()` refs).
pub fn dependencies_from_file(source_path: &Path) -> Result<Vec<String>> {
    let css_content = fs::read_to_string(source_path)?;
    dependencies_from_string(&css_content)
}
/// Parses CSS with `lightningcss` and visits all rules and URLs to collect dependencies.
/// Filters out external URLs (`http://`, `data:`, etc.) and strips query/fragment suffixes.
pub fn dependencies_from_string(css_content: &str) -> Result<Vec<String>> {
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

    // Create a single visitor to collect both URL and @import dependencies
    let mut visitor = CssDependencyVisitor::new();

    stylesheet
        .visit(&mut visitor)
        .map_err(|_| Error::DependencyExtract {
            message: "CSS dependency visiting failed".to_string(),
        })?;

    let dependencies: Vec<String> = visitor
        .dependencies
        .into_iter()
        .filter(|dep| !dep.is_empty())
        .collect();

    Ok(dependencies)
}

struct CssDependencyVisitor {
    dependencies: Vec<String>,
}

impl CssDependencyVisitor {
    fn new() -> Self {
        Self {
            dependencies: Vec::new(),
        }
    }

    fn add_dependency(&mut self, url: &str) {
        if is_external_url(url) {
            return;
        }

        // Remove URL fragments and query parameters
        let clean_url = url.split(['?', '#']).next().unwrap_or(url).to_string();

        if !self.dependencies.contains(&clean_url) {
            self.dependencies.push(clean_url);
        }
    }
}

impl<'i> Visitor<'i> for CssDependencyVisitor {
    type Error = ();

    fn visit_url(&mut self, url: &mut Url<'i>) -> std::result::Result<(), ()> {
        let url_str = url.url.to_string();
        self.add_dependency(&url_str);
        Ok(())
    }

    fn visit_rule(&mut self, rule: &mut CssRule<'i>) -> std::result::Result<(), ()> {
        if let CssRule::Import(import_rule) = rule {
            let url_str = import_rule.url.to_string();
            self.add_dependency(&url_str);
        }
        // IMPORTANT: lightningcss does NOT auto-recurse into nested rules (@media, @supports,
        // nesting). Without this call, url() values inside nested blocks would be missed.
        rule.visit_children(self)
    }

    fn visit_types(&self) -> VisitTypes {
        lightningcss::visit_types!(URLS | RULES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_rule() {
        let css = r#"@import url("chrome://global/skin/design-system/tokens.css");"#;
        let deps = dependencies_from_string(css).unwrap();
        assert!(
            deps.contains(&"chrome://global/skin/design-system/tokens.css".to_string()),
            "Expected tokens.css in deps: {:?}",
            deps
        );
    }

    #[test]
    fn test_url_in_simple_rule() {
        let css = r#"
.handle {
  background-image: url("chrome://global/skin/icons/move-16.svg");
}
"#;
        let deps = dependencies_from_string(css).unwrap();
        assert!(
            deps.contains(&"chrome://global/skin/icons/move-16.svg".to_string()),
            "Expected move-16.svg in deps: {:?}",
            deps
        );
    }
}
