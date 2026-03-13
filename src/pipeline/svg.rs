//! Rewrites Firefox-specific SVG `fill="context-fill"` attributes to `fill="black"`
//! so SVGs work as CSS mask sources outside Firefox.

use regex::Regex;
use std::sync::LazyLock;

/// Rewrites Firefox-specific SVG fill attributes for cross-browser compatibility.
///
/// Firefox uses `fill="context-fill"` and `fill-opacity="context-fill-opacity"` to allow
/// CSS to control SVG icon colors via `-moz-context-properties`. Outside Firefox, these
/// values are invalid and cause icons to be invisible.
///
/// This rewrites them to `fill="black"` (opaque, works as mask source) and removes
/// `fill-opacity="context-fill-opacity"` (defaults to 1).
pub fn transform_svg_context_fill(svg: &str) -> String {
    // Replace fill="context-fill ..." variants with fill="black"
    static FILL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"fill="context-fill[^"]*""#).unwrap());
    // Remove fill-opacity="context-fill-opacity"
    static FILL_OPACITY_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\s*fill-opacity="context-fill-opacity""#).unwrap());

    let result = FILL_RE.replace_all(svg, r#"fill="black""#);
    let result = FILL_OPACITY_RE.replace_all(&result, "");
    result.into_owned()
}
