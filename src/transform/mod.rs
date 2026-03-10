//! File transformation: rewrites imports, URLs, and Firefox-specific patterns in JS, CSS,
//! and markdown files so they work as standalone web components outside Firefox.

pub mod css;
pub mod js;
pub mod markdown;

pub(crate) mod css_transform;
pub(crate) mod js_transform;
