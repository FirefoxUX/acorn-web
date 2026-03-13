//! URL classification helpers and `chrome://` URL replacement used by both JS and CSS
//! transform pipelines.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Returns `true` for URLs that point outside the Firefox source tree and should
/// not be resolved or rewritten (data URIs, HTTP(S), protocol-relative).
pub fn is_external_url(url: &str) -> bool {
    url.starts_with("data:")
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("//")
}

/// Returns true if the URL is a Firefox-internal URL that cannot be resolved
/// outside of the Firefox build system (e.g., privileged XPCOM/Gecko modules).
pub fn is_unresolvable_firefox_url(url: &str) -> bool {
    url.starts_with("resource://gre/modules/")
}

/// Replaces `chrome://` URLs in the given code string using the provided replacement map.
/// Handles fragment identifiers (#) and query strings (?) by stripping them before lookup
/// and re-appending after replacement. Returns the code unchanged if no chrome:// URLs are present.
pub fn replace_chrome_urls(code: &str, url_replacements: &HashMap<String, String>) -> String {
    if !code.contains("chrome://") {
        return code.to_string();
    }

    static CHROME_URL_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"chrome://[^"'\s\)>}]+"#).unwrap());

    CHROME_URL_RE
        .replace_all(code, |caps: &regex::Captures| {
            let url = &caps[0];

            if let Some(replacement) = url_replacements.get(url) {
                return replacement.clone();
            }

            // Try splitting at fragment identifier (#) or query string (?)
            if let Some(idx) = url.find(['?', '#']) {
                let base = &url[..idx];
                let suffix = &url[idx..];
                if let Some(replacement) = url_replacements.get(base) {
                    return format!("{}{}", replacement, suffix);
                }
            }

            url.to_string()
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_external_url() {
        assert!(is_external_url("https://example.com"));
        assert!(is_external_url("http://example.com"));
        assert!(is_external_url("data:image/png;base64,abc"));
        assert!(is_external_url("//cdn.example.com/foo.js"));
        assert!(!is_external_url("chrome://global/skin/foo.css"));
        assert!(!is_external_url("./relative.mjs"));
    }

    #[test]
    fn test_is_unresolvable_firefox_url() {
        assert!(is_unresolvable_firefox_url(
            "resource://gre/modules/AppConstants.sys.mjs"
        ));
        assert!(!is_unresolvable_firefox_url("resource://gre/skin/foo.css"));
        assert!(!is_unresolvable_firefox_url(
            "chrome://global/content/foo.mjs"
        ));
    }

    #[test]
    fn test_replace_chrome_urls_no_chrome() {
        let map = HashMap::new();
        let code = "body { color: red; }";
        assert_eq!(replace_chrome_urls(code, &map), code);
    }

    #[test]
    fn test_replace_chrome_urls_direct_match() {
        let mut map = HashMap::new();
        map.insert(
            "chrome://global/skin/icon.svg".to_string(),
            "../assets/icon.svg".to_string(),
        );
        let code = r#"url("chrome://global/skin/icon.svg")"#;
        let result = replace_chrome_urls(code, &map);
        assert_eq!(result, r#"url("../assets/icon.svg")"#);
    }

    #[test]
    fn test_replace_chrome_urls_with_fragment() {
        let mut map = HashMap::new();
        map.insert(
            "chrome://global/skin/star.svg".to_string(),
            "../assets/star.svg".to_string(),
        );
        let code = r#"url("chrome://global/skin/star.svg#full")"#;
        let result = replace_chrome_urls(code, &map);
        assert_eq!(result, r#"url("../assets/star.svg#full")"#);
    }

    #[test]
    fn test_replace_chrome_urls_no_match_passthrough() {
        let map = HashMap::new();
        let code = r#"url("chrome://unknown/skin/missing.svg")"#;
        let result = replace_chrome_urls(code, &map);
        assert_eq!(result, code);
    }
}
