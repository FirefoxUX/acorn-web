//! Rewrites `url()` values in CSS properties (e.g. `background-image`, `content`) from
//! `chrome://` to local relative paths via the `lightningcss` visitor API.

use lightningcss::stylesheet::StyleSheet;
use lightningcss::values::url::Url;
use lightningcss::visitor::{Visit, VisitTypes, Visitor};
use std::collections::HashMap;

use crate::errors::Error;
use crate::utils::url::is_external_url;

/// Rewrites `url()` values in CSS properties (e.g. `background-image`, `content`)
/// from `chrome://` to local relative paths. Preserves query strings and fragment
/// identifiers (e.g. `icon.svg#bar` → `../assets/icon.svg#bar`).
pub struct UrlReplacer<'a> {
    url_replacements: &'a HashMap<String, String>,
}

impl<'a> UrlReplacer<'a> {
    pub fn new(url_replacements: &'a HashMap<String, String>) -> Self {
        Self { url_replacements }
    }

    /// Visits all `url()` values in the stylesheet, replacing their URLs in-place.
    pub fn build(&self, stylesheet: &mut StyleSheet) -> Result<(), Error> {
        let mut visitor = UrlReplacerVisitor {
            url_replacements: self.url_replacements,
        };
        stylesheet
            .visit(&mut visitor)
            .map_err(|e| Error::CssTransform {
                message: format!("{:?}", e),
            })
    }
}

struct UrlReplacerVisitor<'a> {
    url_replacements: &'a HashMap<String, String>,
}

impl<'a, 'i> Visitor<'i> for UrlReplacerVisitor<'a> {
    type Error = Error;

    fn visit_url(&mut self, url: &mut Url<'i>) -> std::result::Result<(), Self::Error> {
        let url_str = url.url.to_string();

        // Preserve query strings and fragment identifiers across replacement:
        // e.g. chrome://global/skin/icon.svg#star -> ../assets/icon.svg#star
        // The replacement map only contains base URLs (without ?/# suffixes).
        let (base, suffix) = match url_str.find(['?', '#']) {
            Some(idx) => (&url_str[..idx], &url_str[idx..]),
            None => (url_str.as_str(), ""),
        };

        if let Some(replacement) = self.url_replacements.get(base) {
            // Reconstruct the url with the replacement and the original suffix
            let new_url = format!("{}{}", replacement, suffix);
            url.url = new_url.into();
        } else if !is_external_url(base) {
            return Err(Error::UrlNotFound { url: url_str });
        }
        Ok(())
    }

    fn visit_types(&self) -> VisitTypes {
        lightningcss::visit_types!(URLS | RULES)
    }
}
