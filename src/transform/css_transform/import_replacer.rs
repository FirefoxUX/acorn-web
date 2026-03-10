//! Rewrites `@import url("chrome://...")` rules to use local relative paths
//! via the `lightningcss` visitor API.

use lightningcss::stylesheet::StyleSheet;
use lightningcss::visitor::{Visit, VisitTypes, Visitor};
use std::collections::HashMap;

use crate::errors::Error;
use crate::utils::url::is_external_url;

/// Rewrites `@import` rule URLs in a parsed stylesheet using the `lightningcss` visitor.
/// Errors on unrecognized non-external URLs to catch missing mappings early.
pub struct ImportReplacer<'a> {
    url_replacements: &'a HashMap<String, String>,
}

impl<'a> ImportReplacer<'a> {
    pub fn new(url_replacements: &'a HashMap<String, String>) -> Self {
        Self { url_replacements }
    }

    /// Visits all `@import` rules in the stylesheet, replacing their URLs in-place.
    pub fn build(&self, stylesheet: &mut StyleSheet) -> Result<(), Error> {
        let mut visitor = ImportReplacerVisitor {
            url_replacements: self.url_replacements,
        };
        stylesheet
            .visit(&mut visitor)
            .map_err(|e| Error::CssTransform {
                message: format!("{:?}", e),
            })
    }
}

struct ImportReplacerVisitor<'a> {
    url_replacements: &'a HashMap<String, String>,
}

impl<'a, 'i> Visitor<'i> for ImportReplacerVisitor<'a> {
    type Error = Error;

    // Note: no visit_children() needed here because @import rules are always top-level
    // (CSS spec forbids @import inside other at-rules or nested blocks).
    fn visit_rule(
        &mut self,
        rule: &mut lightningcss::rules::CssRule<'i>,
    ) -> std::result::Result<(), Self::Error> {
        if let lightningcss::rules::CssRule::Import(import_rule) = rule {
            let url_str = import_rule.url.to_string();
            if let Some(replacement) = self.url_replacements.get(&url_str) {
                import_rule.url = replacement.clone().into();
            } else if !is_external_url(&url_str) {
                return Err(Error::UrlNotFound { url: url_str });
            }
        }
        Ok(())
    }

    fn visit_types(&self) -> VisitTypes {
        lightningcss::visit_types!(URLS | RULES)
    }
}
