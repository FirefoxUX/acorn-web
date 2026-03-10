//! String-based CSS post-processor that converts Firefox's `-moz-context-properties` icon
//! coloring mechanism to standard `mask-image` CSS. Operates on serialized CSS text because
//! `-moz-context-properties` is not recognized by lightningcss's typed AST.

use regex::Regex;
use std::sync::LazyLock;

/// Transforms CSS rules that use Firefox's `-moz-context-properties` mechanism
/// to use standard CSS `mask-image` for cross-browser SVG icon coloring.
///
/// This is a string-based post-processing step run after lightningcss serialization
/// because coordinating changes across multiple declarations in a rule is simpler
/// with string manipulation than with lightningcss's visitor pattern for custom properties.
pub fn transform_icon_properties(css: &str) -> String {
    let blocks = parse_css_blocks(css);
    let mut result = String::with_capacity(css.len());

    for block in blocks {
        match block {
            CssBlock::Text(text) => result.push_str(text),
            CssBlock::DeclarationBlock { before, declarations, after } => {
                result.push_str(before);
                let transformed = transform_declaration_block(declarations);
                result.push_str(&transformed);
                result.push_str(after);
            }
        }
    }

    result
}

/// A parsed CSS block — either raw text (selectors, at-rules) or a declaration block.
enum CssBlock<'a> {
    Text(&'a str),
    DeclarationBlock {
        before: &'a str,    // The opening `{`
        declarations: &'a str, // Content between braces
        after: &'a str,     // The closing `}`
    },
}

/// Parse CSS into blocks by tracking brace depth.
/// Only the innermost declaration blocks (depth 0→1) are extracted for transformation.
/// Nested rules (depth > 1) are handled recursively by the transform.
fn parse_css_blocks(css: &str) -> Vec<CssBlock<'_>> {
    let mut blocks = Vec::new();
    let mut depth: i32 = 0;
    let mut last_pos = 0;
    let mut block_start = 0;

    let bytes = css.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip string literals
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            i += 1; // skip closing quote
            continue;
        }

        // Skip comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }

        if bytes[i] == b'{' {
            if depth == 0 {
                // Push any text before this block
                if last_pos < i + 1 {
                    blocks.push(CssBlock::Text(&css[last_pos..i + 1]));
                }
                block_start = i + 1;
            }
            depth += 1;
        } else if bytes[i] == b'}' {
            depth -= 1;
            if depth == 0 {
                let declarations = &css[block_start..i];
                blocks.push(CssBlock::DeclarationBlock {
                    before: "",
                    declarations,
                    after: "}",
                });
                last_pos = i + 1;
            }
        }
        i += 1;
    }

    // Push any remaining text
    if last_pos < css.len() {
        blocks.push(CssBlock::Text(&css[last_pos..]));
    }

    blocks
}

/// Transform a declaration block's content. This handles nested rules recursively.
fn transform_declaration_block(block_content: &str) -> String {
    // First, check if this block or any nested blocks contain -moz-context-properties
    // or background-image with .svg references
    let has_moz_context = block_content.contains("-moz-context-properties");
    let has_svg_bg = has_svg_background_image(block_content);
    let has_svg_content = has_svg_content_url(block_content);

    if !has_moz_context && !has_svg_bg && !has_svg_content {
        return block_content.to_string();
    }

    // Split declarations from nested rules
    let (own_declarations, nested_content) = split_declarations_and_nested(block_content);

    let transformed_own = if !own_declarations.trim().is_empty() {
        transform_own_declarations(&own_declarations, has_svg_bg, has_svg_content)
    } else {
        own_declarations.clone()
    };

    // Recursively handle nested content
    let transformed_nested = if !nested_content.is_empty() {
        transform_nested_content(&nested_content)
    } else {
        nested_content
    };

    format!("{}{}", transformed_own, transformed_nested)
}

/// Check if a CSS block contains background-image with an .svg URL reference
fn has_svg_background_image(css: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"background-image\s*:[^;]*\.svg[^;]*;"#).unwrap()
    });
    RE.is_match(css)
}

/// Check if a CSS block contains content: url() with an .svg reference
fn has_svg_content_url(css: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"content\s*:\s*url\([^)]*\.svg[^)]*\)"#).unwrap()
    });
    RE.is_match(css)
}

/// Split a declaration block into own declarations (before any nested `{ }` blocks)
/// and the rest (nested rules). This is a simplified split.
fn split_declarations_and_nested(content: &str) -> (String, String) {
    let mut depth = 0;
    let mut first_nested_start = None;

    let bytes = content.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip strings
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' { i += 1; }
                i += 1;
            }
            i += 1;
            continue;
        }
        // Skip comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }

        if bytes[i] == b'{' {
            if depth == 0 && first_nested_start.is_none() {
                // Find start of this nested rule's selector (go back to find line start)
                let mut sel_start = i;
                while sel_start > 0 {
                    // Walk backwards to find start of the selector line
                    if content[..sel_start].ends_with('\n') || content[..sel_start].ends_with(';') || content[..sel_start].ends_with('}') {
                        break;
                    }
                    sel_start -= 1;
                }
                first_nested_start = Some(sel_start);
            }
            depth += 1;
        } else if bytes[i] == b'}' {
            depth -= 1;
        }
        i += 1;
    }

    match first_nested_start {
        Some(pos) => (content[..pos].to_string(), content[pos..].to_string()),
        None => (content.to_string(), String::new()),
    }
}

/// Transform the own declarations of a block (not nested rules).
fn transform_own_declarations(declarations: &str, _has_svg_bg_parent: bool, _has_svg_content_parent: bool) -> String {
    let has_moz_context = declarations.contains("-moz-context-properties");
    let has_color_prop = HAS_COLOR_RE.is_match(declarations);
    let has_bg_svg = has_svg_background_image(declarations);
    let has_content_svg = has_svg_content_url(declarations);

    let mut result = declarations.to_string();

    // A. Convert background-image: url(*.svg) to mask-image
    if has_bg_svg {
        result = convert_background_to_mask(&result);
    }

    // B. Convert content: url(*.svg) to mask approach (always, for cross-browser icon coloring)
    if has_content_svg {
        result = convert_content_to_mask(&result);
    }

    // C. Handle -moz-context-properties rules
    if has_moz_context {
        // Remove -moz-context-properties
        result = remove_moz_context_properties(&result);

        // Convert fill/stroke based on context
        if has_bg_svg || has_content_svg {
            // Has background/content SVG: fill → background-color
            result = convert_fill_to_background_color(&result);
        } else if has_color_prop {
            // Has color already: just remove fill (color provides the value)
            result = remove_fill_declarations(&result);
        } else {
            // No background-image, no color: fill → color
            result = convert_fill_to_color(&result);
        }

        // Remove stroke and fill-opacity
        result = remove_stroke_declarations(&result);
        result = remove_fill_opacity_declarations(&result);
    }

    result
}

/// Recursively transform nested CSS content
fn transform_nested_content(content: &str) -> String {
    // Re-parse nested content through transform_icon_properties
    // since nested rules can also contain -moz-context-properties
    let mut result = String::with_capacity(content.len());
    let mut depth: i32 = 0;
    let mut last_end = 0;
    let mut block_start = 0;

    let bytes = content.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip strings
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' { i += 1; }
                i += 1;
            }
            i += 1;
            continue;
        }
        // Skip comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }

        if bytes[i] == b'{' {
            if depth == 0 {
                block_start = i + 1;
            }
            depth += 1;
        } else if bytes[i] == b'}' {
            depth -= 1;
            if depth == 0 {
                // Push selector text
                result.push_str(&content[last_end..block_start]);
                // Recursively transform the nested block content
                let inner = &content[block_start..i];
                let transformed = transform_declaration_block(inner);
                result.push_str(&transformed);
                result.push('}');
                last_end = i + 1;
            }
        }
        i += 1;
    }

    // Push any remaining text
    if last_end < content.len() {
        result.push_str(&content[last_end..]);
    }

    result
}

// --- Regex-based property transformations ---

static HAS_COLOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Matches `color:` but NOT `-moz-context-properties:` or `background-color:`
    Regex::new(r"(?m)^\s*color\s*:").unwrap()
});

static MOZ_CONTEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*-moz-context-properties\s*:[^;]*;\s*\n?").unwrap()
});

static BG_IMAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(\s*)background-image(\s*:\s*[^;]*\.svg[^;]*;)").unwrap()
});

static BG_SIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(\s*)background-size(\s*:[^;]*;)").unwrap()
});

static BG_REPEAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(\s*)background-repeat(\s*:[^;]*;)").unwrap()
});

static BG_POSITION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(\s*)background-position(\s*:[^;]*;)").unwrap()
});

static FILL_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Matches `fill: <value>;` but NOT `fill-opacity:` or `-moz-context-properties: fill`
    Regex::new(r"(?m)^(\s*)fill(\s*:\s*)([^;]+)(;\s*\n?)").unwrap()
});

static STROKE_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*stroke\s*:[^;]*;\s*\n?").unwrap()
});

static FILL_OPACITY_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*fill-opacity\s*:[^;]*;\s*\n?").unwrap()
});

static CONTENT_URL_SVG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^(\s*)content(\s*:\s*)url\(([^)]*\.svg[^)]*)\)(\s*;\s*\n?)"#).unwrap()
});

fn remove_moz_context_properties(css: &str) -> String {
    MOZ_CONTEXT_RE.replace_all(css, "").into_owned()
}

fn convert_background_to_mask(css: &str) -> String {
    let mut result = css.to_string();

    // background-image → mask-image (with -webkit- prefix)
    result = BG_IMAGE_RE.replace_all(&result, |caps: &regex::Captures| {
        let indent = &caps[1];
        let value = &caps[2];
        format!(
            "{indent}-webkit-mask-image{value}\n{indent}mask-image{value}"
        )
    }).into_owned();

    // background-size → mask-size
    result = BG_SIZE_RE.replace_all(&result, |caps: &regex::Captures| {
        let indent = &caps[1];
        let value = &caps[2];
        format!(
            "{indent}-webkit-mask-size{value}\n{indent}mask-size{value}"
        )
    }).into_owned();

    // background-repeat → mask-repeat
    result = BG_REPEAT_RE.replace_all(&result, |caps: &regex::Captures| {
        let indent = &caps[1];
        let value = &caps[2];
        format!(
            "{indent}-webkit-mask-repeat{value}\n{indent}mask-repeat{value}"
        )
    }).into_owned();

    // background-position → mask-position
    result = BG_POSITION_RE.replace_all(&result, |caps: &regex::Captures| {
        let indent = &caps[1];
        let value = &caps[2];
        format!(
            "{indent}-webkit-mask-position{value}\n{indent}mask-position{value}"
        )
    }).into_owned();

    result
}

fn convert_content_to_mask(css: &str) -> String {
    CONTENT_URL_SVG_RE.replace_all(css, |caps: &regex::Captures| {
        let indent = &caps[1];
        let url_value = &caps[3]; // the URL path inside url()
        format!(
            "{indent}content: \"\";\n\
             {indent}-webkit-mask-image: url({url_value});\n\
             {indent}mask-image: url({url_value});\n\
             {indent}-webkit-mask-size: contain;\n\
             {indent}mask-size: contain;\n\
             {indent}background-color: currentColor;\n"
        )
    }).into_owned()
}

fn convert_fill_to_background_color(css: &str) -> String {
    FILL_DECL_RE.replace_all(css, |caps: &regex::Captures| {
        let indent = &caps[1];
        let separator = &caps[2];
        let value = &caps[3];
        let end = &caps[4];
        format!("{indent}background-color{separator}{value}{end}")
    }).into_owned()
}

fn convert_fill_to_color(css: &str) -> String {
    FILL_DECL_RE.replace_all(css, |caps: &regex::Captures| {
        let indent = &caps[1];
        let separator = &caps[2];
        let value = &caps[3];
        let end = &caps[4];
        format!("{indent}color{separator}{value}{end}")
    }).into_owned()
}

fn remove_fill_declarations(css: &str) -> String {
    FILL_DECL_RE.replace_all(css, "").into_owned()
}

fn remove_stroke_declarations(css: &str) -> String {
    STROKE_DECL_RE.replace_all(css, "").into_owned()
}

fn remove_fill_opacity_declarations(css: &str) -> String {
    FILL_OPACITY_DECL_RE.replace_all(css, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_image_to_mask() {
        let input = r#".rating-star {
  background-image: url("../../assets/rating-star.svg#empty");
  background-position: center;
  background-repeat: no-repeat;
  background-size: 16px 16px;
  fill: var(--icon-color);
  -moz-context-properties: fill;
}"#;
        let result = transform_icon_properties(input);
        assert!(result.contains("mask-image"));
        assert!(result.contains("-webkit-mask-image"));
        assert!(result.contains("mask-size"));
        assert!(result.contains("mask-repeat"));
        assert!(result.contains("mask-position"));
        assert!(result.contains("background-color: var(--icon-color)"));
        assert!(!result.contains("background-image"));
        assert!(!result.contains("-moz-context-properties"));
        assert!(!result.contains("fill:"));
    }

    #[test]
    fn test_context_properties_without_background() {
        let input = r#".icon {
  -moz-context-properties: fill, stroke;
  fill: currentColor;
  stroke: currentColor;
  color: var(--icon-color);
}"#;
        let result = transform_icon_properties(input);
        assert!(!result.contains("-moz-context-properties"));
        assert!(!result.contains("fill:"));
        assert!(!result.contains("stroke:"));
        assert!(result.contains("color: var(--icon-color)"));
    }

    #[test]
    fn test_fill_to_color_without_existing_color() {
        let input = r#".chevron-icon, #heading-icon {
  -moz-context-properties: fill;
  fill: currentColor;
  width: 16px;
}"#;
        let result = transform_icon_properties(input);
        assert!(!result.contains("-moz-context-properties"));
        assert!(result.contains("color: currentColor"));
        assert!(result.contains("width: 16px"));
    }

    #[test]
    fn test_content_url_to_mask() {
        let input = r#"&:not(:last-child):after {
  content: url("../assets/arrow-right-12.svg");
  display: inline-flex;
  height: var(--breadcrumb-icon-size);
  -moz-context-properties: fill;
  fill: currentColor;
}"#;
        let result = transform_icon_properties(input);
        assert!(result.contains(r#"content: "";"#));
        assert!(result.contains("mask-image: url(\"../assets/arrow-right-12.svg\")"));
        assert!(result.contains("-webkit-mask-image"));
        assert!(result.contains("mask-size: contain"));
        assert!(result.contains("background-color: currentColor"));
        assert!(!result.contains("-moz-context-properties"));
    }

    #[test]
    fn test_no_transform_without_svg_or_context() {
        let input = r#".normal {
  background-image: url("pattern.png");
  color: red;
}"#;
        let result = transform_icon_properties(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_nested_rule_with_context_properties() {
        let input = r#".parent {
  & .child {
    background-image: url("../../assets/icon.svg");
    -moz-context-properties: fill;
    fill: currentColor;
  }
}"#;
        let result = transform_icon_properties(input);
        assert!(result.contains("mask-image"));
        assert!(result.contains("background-color: currentColor"));
        assert!(!result.contains("-moz-context-properties"));
    }

    #[test]
    fn test_var_fallback_background_image() {
        let input = r#".chevron {
  background-image: var(--icon, url("../../assets/arrow.svg"));
}"#;
        let result = transform_icon_properties(input);
        assert!(result.contains("mask-image"));
        assert!(!result.contains("background-image"));
    }

    #[test]
    fn test_fill_and_stroke_removal() {
        let input = r#"& img {
  -moz-context-properties: fill, fill-opacity, stroke;
  fill: var(--button-icon-fill);
  stroke: var(--button-icon-stroke);
  fill-opacity: 0.5;
}"#;
        let result = transform_icon_properties(input);
        assert!(!result.contains("-moz-context-properties"));
        assert!(!result.contains("stroke:"));
        assert!(!result.contains("fill-opacity:"));
        assert!(result.contains("color: var(--button-icon-fill)"));
    }
}
