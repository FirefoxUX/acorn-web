//! JS transformation pipeline. Runs `oxc` AST transformers (import rewriting, CSS inlining,
//! icon transforms) then applies string-based post-processing for patterns that can't be
//! handled at the AST level (custom element guarding, inline CSS icon properties, img->acorn-icon).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use oxc::allocator::Allocator;
use oxc::parser::{Parser, ParserReturn};
use oxc::semantic::{SemanticBuilder, SemanticBuilderReturn};
use oxc::span::SourceType;
use oxc_codegen::Codegen;
use oxc_traverse::ReusableTraverseCtx;

use crate::errors::{Error, Result};
use crate::pipeline::fluent::FtlMap;
use crate::transform::css_transform::icon_property_transform;
use crate::transform::js_transform::{
    CssInlineTransformer, IconTemplateImportTransformer, ImportCssTransformer, UrlTransformer,
};
use crate::utils::url::replace_chrome_urls;

/// Reads a JS file and applies all transformations (import rewriting, CSS inlining, icon
/// conversion, custom element guarding, chrome:// URL replacement).
pub fn transform_from_file(
    source_path: &Path,
    url_replacements: &HashMap<String, String>,
    css_replacements: Option<&HashMap<String, String>>,
    ftl_map: &FtlMap,
    fluent_fallbacks: &HashMap<String, String>,
) -> Result<String> {
    let source_code = fs::read_to_string(source_path)?;
    transform_from_string(&source_code, url_replacements, css_replacements, ftl_map, fluent_fallbacks)
}

/// Transforms JS source code through two phases:
/// 1. **AST phase** (`oxc`): parse -> semantic analysis -> traverse with transformers -> codegen
/// 2. **String phase**: post-process the codegen output for patterns that require regex
///    (custom element guards, inline CSS icon props, img->acorn-icon, chrome:// URLs)
pub fn transform_from_string(
    source_code: &str,
    url_replacements: &HashMap<String, String>,
    css_replacements: Option<&HashMap<String, String>>,
    ftl_map: &FtlMap,
    fluent_fallbacks: &HashMap<String, String>,
) -> Result<String> {
    // --- Phase 1: Parse source into AST ---
    // oxc uses an arena allocator — all AST nodes live in `allocator` and are freed together.
    let allocator = Allocator::default();
    let source_type = SourceType::default().with_module(true);
    let parser = Parser::new(&allocator, source_code, source_type);
    let ParserReturn {
        mut program,
        errors: _parser_errors,
        panicked,
        ..
    } = parser.parse();

    if panicked {
        return Err(Error::JsPanicParse);
    }

    // Semantic analysis produces scope/binding information that `oxc_traverse` needs
    // to safely rename or inject identifiers without collisions.
    let SemanticBuilderReturn {
        semantic,
        errors: semantic_errors,
    } = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .with_build_jsdoc(false)
        .with_cfg(false)
        .build(&program);

    if !semantic_errors.is_empty() {
        let error_messages: Vec<String> =
            semantic_errors.iter().map(|e| format!("{:?}", e)).collect();
        return Err(Error::JsParse {
            message: format!("Semantic errors: {}", error_messages.join(", ")),
        });
    }
    let scoping = semantic.into_scoping();
    let mut ctx = ReusableTraverseCtx::new((), scoping, &allocator);

    // --- Phase 1b: AST transformations via oxc_traverse ---
    // Order matters: inline CSS first (removes <link> tags, adds static styles),
    // then add `css` import if inlining happened, then rewrite all import URLs,
    // then replace chrome:// in HTML template attributes with new URL() expressions.
    if let Some(css_replacements) = css_replacements {
        let made_replacements =
            CssInlineTransformer::new(css_replacements).build(&mut program, &mut ctx);
        if made_replacements {
            ImportCssTransformer::new().build(&mut program, &mut ctx);
        }
    }
    UrlTransformer::new(url_replacements).build(&mut program, &mut ctx)?;
    IconTemplateImportTransformer::new(url_replacements).build(&mut program, &mut ctx);

    // --- Phase 1c: Serialize AST back to JS source ---
    let codegen = Codegen::new();
    let output = codegen.build(&program);
    let output = output.code.replace("\t", "  ");

    // --- Phase 2: String-based post-processing ---
    // These transforms use regex on the codegen output because they target patterns
    // that are easier to match as text (custom element registrations, CSS-in-JS
    // icon properties, HTML <img> tags) than to express as AST visitors.
    let output = guard_custom_element_definitions(&output);

    // Transform -moz-context-properties in inline CSS template literals
    let output = transform_inline_css_icon_properties(&output);

    // Transform <img> to <acorn-icon> for SVG icons and update CSS selectors
    let output = transform_img_to_acorn_icon(&output);

    // Replace any remaining chrome:// URLs that weren't handled by the AST-based
    // UrlTransformer (which only handles import declarations). This catches chrome://
    // URLs in string literals, template literal HTML attributes, inline CSS, etc.
    let output = replace_chrome_urls(&output, url_replacements);

    // Remove `(window.)?MozXULElement.insertFTLIfNeeded(...)` calls entirely —
    // components no longer auto-load FTL files; consumers set up Fluent themselves.
    let output = remove_moz_xul_element_calls(&output);

    // Add English fallback attributes alongside data-l10n-id for zero-setup rendering
    let output = add_fluent_fallbacks(&output, ftl_map, fluent_fallbacks);

    // Guard programmatic document.l10n calls so they're no-ops without Fluent
    let output = guard_document_l10n_calls(&output, ftl_map);

    Ok(output)
}

/// Wraps `customElements.define(...)` calls with a guard to prevent
/// "has already been defined" errors when the module is imported multiple times.
///
/// Transforms:
///   `customElements.define("moz-foo", MozFoo);`
/// Into:
///   `if (!customElements.get("moz-foo")) { customElements.define("moz-foo", MozFoo); }`
fn guard_custom_element_definitions(code: &str) -> String {
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"customElements\.define\(("[^"]+"),\s*(.+?)\);"#).unwrap()
    });

    RE.replace_all(code, r#"if (!customElements.get($1)) { customElements.define($1, $2); }"#)
        .into_owned()
}

/// Transforms `-moz-context-properties` icon patterns inside `css\`...\`` tagged template
/// literals in JS files. This handles CSS that was written directly in component source
/// (not imported from external CSS files, which are already transformed by css.rs).
fn transform_inline_css_icon_properties(code: &str) -> String {
    static CSS_TEMPLATE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        // Match css`...` tagged template literals. Captures the CSS content between backticks.
        // Uses a non-greedy match to handle multiple css`` in the same file.
        regex::Regex::new(r"css`").unwrap()
    });

    if !code.contains("-moz-context-properties") {
        return code.to_string();
    }

    // Also transform <style>...</style> blocks in html`` templates
    static STYLE_TAG_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?s)<style[^>]*>(.*?)</style>").unwrap()
    });

    let mut result = String::with_capacity(code.len());
    let mut search_start = 0;

    // First pass: css`` template literals
    while let Some(m) = CSS_TEMPLATE_RE.find(&code[search_start..]) {
        let content_start = search_start + m.end(); // position right after the backtick

        // Find the matching closing backtick, handling ${} expressions
        if let Some(end_pos) = find_template_literal_end(code, content_start) {
            let css_content = &code[content_start..end_pos];

            // Only transform if this CSS contains relevant patterns
            if css_content.contains("-moz-context-properties")
                || (css_content.contains(".svg") && css_content.contains("background-image"))
                || (css_content.contains(".svg") && css_content.contains("content:"))
            {
                result.push_str(&code[search_start..content_start]);
                let transformed = icon_property_transform::transform_icon_properties(css_content);
                result.push_str(&transformed);
                search_start = end_pos;
            } else {
                // No relevant patterns, keep as-is up to after the closing backtick
                result.push_str(&code[search_start..end_pos + 1]);
                search_start = end_pos + 1;
            }
        } else {
            // Couldn't find closing backtick, keep the rest as-is
            break;
        }
    }

    result.push_str(&code[search_start..]);

    // Second pass: <style>...</style> blocks in html`` templates
    if result.contains("-moz-context-properties") && result.contains("<style") {
        result = STYLE_TAG_RE
            .replace_all(&result, |caps: &regex::Captures| {
                let css_content = &caps[1];
                if css_content.contains("-moz-context-properties")
                    || (css_content.contains(".svg") && css_content.contains("background-image"))
                    || (css_content.contains(".svg") && css_content.contains("content:"))
                {
                    let full_match = &caps[0];
                    let transformed =
                        icon_property_transform::transform_icon_properties(css_content);
                    full_match.replace(css_content, &transformed)
                } else {
                    caps[0].to_string()
                }
            })
            .into_owned();
    }

    result
}

/// Find the end of a template literal (the closing backtick), handling:
/// - `${...}` expressions (which can contain nested template literals)
/// - Escaped backticks `\``
fn find_template_literal_end(code: &str, start: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut i = start;
    let mut depth = 0; // tracks nested ${} depth

    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped char
            continue;
        }

        if depth > 0 {
            // Inside a ${} expression
            if bytes[i] == b'{' {
                depth += 1;
            } else if bytes[i] == b'}' {
                depth -= 1;
            } else if bytes[i] == b'`' {
                // Nested template literal inside ${}
                i += 1;
                // Find end of nested template literal
                if let Some(nested_end) = find_template_literal_end(code, i) {
                    i = nested_end + 1;
                    continue;
                } else {
                    return None;
                }
            }
            i += 1;
            continue;
        }

        // At template literal level
        if bytes[i] == b'`' {
            return Some(i);
        }
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            depth = 1;
            i += 2;
            continue;
        }
        i += 1;
    }

    None
}

/// Transforms `<img>` tags to `<acorn-icon>` in html`` template literals and adds
/// `acorn-icon` alongside `img` in CSS selectors within css`` template literals.
///
/// This enables cross-browser colorable SVG icons. The `<acorn-icon>` component uses
/// CSS mask-image to render SVGs that inherit color from the parent's CSS `color` property.
fn transform_img_to_acorn_icon(code: &str) -> String {
    if !code.contains("<img") {
        return code.to_string();
    }

    // Replace <img ... /> and <img ...></img> with <acorn-icon ...></acorn-icon>
    // in html`` template literals. Since <img> only appears in html templates
    // in these component files, we can safely match across the whole string.

    // Pattern 1: Self-closing <img ... />
    static IMG_SELF_CLOSING: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?s)<img\b((?:[^>]|(?:"[^"]*")|(?:'[^']*'))*?)\s*/>"#).unwrap()
    });

    // Pattern 2: <img ...></img>
    static IMG_EXPLICIT_CLOSE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?s)<img\b((?:[^>]|(?:"[^"]*")|(?:'[^']*'))*?)>\s*</img\s*>"#)
            .unwrap()
    });

    let mut result = IMG_SELF_CLOSING
        .replace_all(code, "<acorn-icon$1></acorn-icon>")
        .into_owned();

    result = IMG_EXPLICIT_CLOSE
        .replace_all(&result, "<acorn-icon$1></acorn-icon>")
        .into_owned();

    // Update CSS selectors: add `acorn-icon` alongside `img` in css`` templates.
    // Matches patterns like `& img {`, `& img,`, `& img ` in CSS contexts.
    if result.contains("acorn-icon") {
        result = transform_css_img_selectors(&result);
    }

    // Add acorn-icon import if any replacements were made
    if result.contains("<acorn-icon") {
        result = add_acorn_icon_import(&result);
    }

    result
}

/// In CSS within css`` template literals, add `acorn-icon` alongside `img` selectors.
/// E.g., `& img {` → `& img, & acorn-icon {`
fn transform_css_img_selectors(code: &str) -> String {
    // Match `& img` followed by `{`, `,`, ` `, or end-of-selector characters
    // in css`` template content. Be careful not to match inside property names
    // like `background-image` or `mask-image`.
    static CSS_IMG_SELECTOR: LazyLock<regex::Regex> = LazyLock::new(|| {
        // Match `& img` as a CSS selector (preceded by `&` or beginning of selector)
        regex::Regex::new(r"(&\s*)img(\s*[,\{])").unwrap()
    });
    // Match `:has(img)` pseudo-class selectors
    static CSS_HAS_IMG: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r":has\(img\)").unwrap());

    let mut result = String::with_capacity(code.len());
    let mut search_start = 0;

    // Only process inside css`` templates
    static CSS_TAG: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"css`").unwrap());

    while let Some(m) = CSS_TAG.find(&code[search_start..]) {
        let content_start = search_start + m.end();

        if let Some(end_pos) = find_template_literal_end(code, content_start) {
            // Push everything up to the css template content
            result.push_str(&code[search_start..content_start]);

            // Transform img selectors in the CSS content
            let css_content = &code[content_start..end_pos];
            let transformed = CSS_IMG_SELECTOR
                .replace_all(css_content, "${1}img, ${1}acorn-icon${2}")
                .into_owned();
            let transformed = CSS_HAS_IMG
                .replace_all(&transformed, ":has(img, acorn-icon)")
                .into_owned();
            result.push_str(&transformed);

            search_start = end_pos;
        } else {
            break;
        }
    }

    result.push_str(&code[search_start..]);
    result
}

/// Removes `(window.)?MozXULElement.insertFTLIfNeeded(...)` calls entirely.
/// Components no longer auto-load FTL files — consumers who want Fluent manage
/// their own setup via `initFluent()` or a custom `document.l10n`.
fn remove_moz_xul_element_calls(code: &str) -> String {
    if !code.contains("MozXULElement") {
        return code.to_string();
    }

    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        // Matches the full statement including optional `window.` prefix, optional chaining,
        // the string argument, closing paren, semicolon, and trailing newline.
        regex::Regex::new(
            r#"[ \t]*(?:window\.)?MozXULElement(?:\?\.|\.)insertFTLIfNeeded\([^)]*\);[ \t]*\n?"#,
        )
        .unwrap()
    });

    RE.replace_all(code, "").into_owned()
}

/// Adds English fallback attributes alongside `data-l10n-id` in HTML templates.
/// When `document.l10n` is absent, static values render. When Fluent is active,
/// it overrides them via `data-l10n-id`.
fn add_fluent_fallbacks(code: &str, ftl_map: &FtlMap, fluent_fallbacks: &HashMap<String, String>) -> String {
    if !code.contains("data-l10n-id") {
        return code.to_string();
    }

    let mut result = code.to_string();

    // --- 3a: Static data-l10n-id="key" (literal strings) ---
    // Match data-l10n-id="key" optionally followed by data-l10n-attrs="list"
    static STATIC_L10N: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"data-l10n-id="([^"]+)"(\s+data-l10n-attrs="([^"]+)")?"#,
        )
        .unwrap()
    });

    result = STATIC_L10N
        .replace_all(&result, |caps: &regex::Captures| {
            let key = &caps[1];
            let has_l10n_attrs = caps.get(3).is_some();
            let l10n_attrs: Vec<&str> = caps
                .get(3)
                .map(|m| m.as_str().split(',').map(|s| s.trim()).collect())
                .unwrap_or_default();

            if let Some(entry) = ftl_map.get(key) {
                let mut fallbacks = Vec::new();

                for (attr, value) in entry {
                    if attr == ".value" {
                        continue; // message value, not an attribute
                    }
                    // If data-l10n-attrs is present, only include those attrs
                    if has_l10n_attrs && !l10n_attrs.contains(&attr.as_str()) {
                        continue;
                    }
                    fallbacks.push(format!("{attr}=\"{value}\""));
                }

                fallbacks.sort(); // deterministic order

                let mut replacement = format!("data-l10n-id=\"{key}\"");
                if !fallbacks.is_empty() {
                    replacement.push(' ');
                    replacement.push_str(&fallbacks.join(" "));
                }
                // Remove data-l10n-attrs (no longer needed)
                replacement
            } else {
                caps[0].to_string()
            }
        })
        .into_owned();

    // --- 3b: Dynamic data-l10n-id=${expr} data-l10n-attrs="attrName" ---
    // Generic: collects ALL FtlMap entries that have the target attribute,
    // builds a lookup map, and replaces data-l10n-attrs with the direct attribute.
    static DYNAMIC_L10N_ATTRS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"data-l10n-id=\$\{([^}]+)\}\s+data-l10n-attrs="([^"]+)""#,
        )
        .unwrap()
    });

    if result.contains("data-l10n-id=${") && result.contains("data-l10n-attrs=") {
        // Collect all attr names referenced by dynamic data-l10n-attrs in this file
        let attr_names: Vec<String> = DYNAMIC_L10N_ATTRS
            .captures_iter(&result)
            .map(|caps| caps[2].to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        // Build a single fallback map from ALL FtlMap entries that have any of the target attrs
        let mut map_entries: Vec<(String, String)> = Vec::new();
        for attr_name in &attr_names {
            for (key, attrs) in ftl_map.iter() {
                if let Some(val) = attrs.get(attr_name.as_str()) {
                    map_entries.push((key.clone(), val.clone()));
                }
            }
        }

        if !map_entries.is_empty() {
            map_entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            map_entries.dedup_by(|(a, _), (b, _)| a == b);

            let entries_str: Vec<String> = map_entries
                .iter()
                .map(|(k, v)| format!("  \"{k}\": \"{v}\""))
                .collect();
            let map_const = format!(
                "const _l10nFallback = {{\n{}\n}};\n",
                entries_str.join(",\n")
            );

            // Inject the constant after the last import
            static LAST_IMPORT: LazyLock<regex::Regex> = LazyLock::new(|| {
                regex::Regex::new(r"(?m)^import\s[^\n]*;\n").unwrap()
            });

            if let Some(m) = LAST_IMPORT.find_iter(&result).last() {
                let insert_pos = m.end();
                result.insert_str(insert_pos, &map_const);
            }

            // Replace: data-l10n-id=${expr} data-l10n-attrs="attrName"
            // With:    data-l10n-id=${expr} attrName=${_l10nFallback[expr] ?? ""}
            result = DYNAMIC_L10N_ATTRS
                .replace_all(&result, |caps: &regex::Captures| {
                    let expr = &caps[1];
                    let attr_name = &caps[2];
                    format!("data-l10n-id=${{{expr}}} {attr_name}=${{_l10nFallback[{expr}] ?? \"\"}}")
                })
                .into_owned();
        }
    }

    // --- 3c: Config-driven fallbacks for dynamic l10n entries with variables ---
    // Matches data-l10n-id with either static "key" or dynamic ${expr} containing
    // a known l10n-id, followed by data-l10n-args. Injects title= from config.
    if !fluent_fallbacks.is_empty() && result.contains("data-l10n-args") {
        // Static key with args: data-l10n-id="key" ... data-l10n-args=...
        // Match the full span from data-l10n-id to end of data-l10n-args
        static STATIC_WITH_ARGS: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(
                r#"data-l10n-id="([^"]+)"\s+(data-l10n-args=\$\{[^}]+\})"#,
            )
            .unwrap()
        });

        result = STATIC_WITH_ARGS
            .replace_all(&result, |caps: &regex::Captures| {
                let key = &caps[1];
                let args_part = &caps[2];
                if let Some(fallback_expr) = fluent_fallbacks.get(key) {
                    format!("title={fallback_expr} data-l10n-id=\"{key}\" {args_part}")
                } else {
                    caps[0].to_string()
                }
            })
            .into_owned();

        // Dynamic key with args: data-l10n-id=${expr containing "key"} ... data-l10n-args=...
        // The expr often looks like: ifDefined(cond ? undefined : "key") or ifDefined(cond ? "key" : undefined)
        static DYNAMIC_WITH_ARGS: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(
                r#"data-l10n-id=\$\{([^}]+)\}\s+(data-l10n-args=\$\{[^}]+\})"#,
            )
            .unwrap()
        });

        result = DYNAMIC_WITH_ARGS
            .replace_all(&result, |caps: &regex::Captures| {
                let expr = &caps[1];
                let args_part = &caps[2];
                // Extract quoted l10n-id from the expression (e.g., "moz-five-star-rating" from ifDefined(...))
                let id_re = regex::Regex::new(r#""([^"]+)""#).unwrap();
                if let Some(id_caps) = id_re.captures(expr) {
                    let key = &id_caps[1];
                    if let Some(fallback_expr) = fluent_fallbacks.get(key) {
                        return format!("title={fallback_expr} data-l10n-id=${{{expr}}} {args_part}");
                    }
                }
                caps[0].to_string()
            })
            .into_owned();
    }

    // --- Warn about unhandled data-l10n-args without fallbacks ---
    static L10N_ARGS_CHECK: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"data-l10n-id=(?:"([^"]+)"|\$\{([^}]+)\})[^>]*?data-l10n-args="#).unwrap()
    });

    for caps in L10N_ARGS_CHECK.captures_iter(&result) {
        let l10n_id = caps.get(1).or(caps.get(2)).map(|m| m.as_str()).unwrap_or("unknown");
        // Check if a title= fallback was already injected before this data-l10n-id
        let match_start = caps.get(0).unwrap().start();
        let preceding = &result[..match_start];
        let line_start = preceding.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_prefix = &result[line_start..match_start];
        if !line_prefix.contains("title=") {
            eprintln!("Warning: data-l10n-args without fallback for l10n-id '{l10n_id}' — add a manual fallback in config.toml [fluent_fallbacks]");
        }
    }

    result
}

/// Guards programmatic `document.l10n` calls so they're no-ops without Fluent.
/// For `document.l10n.setAttributes(el, "id")` calls, injects a textContent fallback
/// from the FtlMap's `.value` entry if available.
fn guard_document_l10n_calls(code: &str, ftl_map: &FtlMap) -> String {
    if !code.contains("document.l10n") {
        return code.to_string();
    }

    let mut result = code.to_string();

    // Generic: find document.l10n.setAttributes(el, "l10n-id") and inject textContent
    // fallback from FtlMap .value before the call.
    static SET_ATTRIBUTES: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"([ \t]*)document\.l10n\.setAttributes\(([^,]+),\s*"([^"]+)"\);"#,
        )
        .unwrap()
    });

    result = SET_ATTRIBUTES
        .replace_all(&result, |caps: &regex::Captures| {
            let indent = &caps[1];
            let element = &caps[2];
            let l10n_id = &caps[3];

            let mut replacement = String::new();

            // If the FtlMap has a .value for this l10n-id, inject a textContent fallback
            if let Some(entry) = ftl_map.get(l10n_id)
                && let Some(value) = entry.get(".value")
            {
                replacement.push_str(&format!(
                    "{indent}if (!{element}.textContent) {{ {element}.textContent = \"{value}\"; }}\n"
                ));
            }

            replacement.push_str(&format!(
                "{indent}if (document.l10n) {{ document.l10n.setAttributes({element}, \"{l10n_id}\"); }}"
            ));

            replacement
        })
        .into_owned();

    // Guard any remaining standalone document.l10n.* calls not already wrapped
    let lines: Vec<&str> = result.lines().collect();
    let mut new_lines = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if line.contains("document.l10n.") && !line.trim().starts_with("//") && !line.trim().starts_with("if") {
            // Check if the previous non-empty line is an `if (document.l10n)` guard
            let prev_significant = lines[..i]
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty());
            // Only consider it already guarded if the previous line is an opening
            // guard block (ends with `{`), not a single-line guarded statement
            let already_guarded = prev_significant
                .map(|l| {
                    (l.contains("if (document.l10n)") || l.contains("if (!document.l10n)"))
                        && l.trim().ends_with('{')
                })
                .unwrap_or(false);
            if !already_guarded {
                let trimmed = line.trim();
                let indent = &line[..line.len() - line.trim_start().len()];
                new_lines.push(format!("{indent}if (document.l10n) {{ {trimmed} }}"));
                continue;
            }
        }
        new_lines.push(line.to_string());
    }
    result = new_lines.join("\n");
    // Preserve trailing newline if original had one
    if code.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    result
}

/// Add an import for acorn-icon.mjs at the top of the file (after existing imports).
fn add_acorn_icon_import(code: &str) -> String {
    // Find the right relative path based on existing import paths in the file
    // Components are at dist/components/moz-*/moz-*.mjs → need ../../dependencies/acorn-icon.mjs
    // Dependencies are at dist/dependencies/*.mjs → need ./acorn-icon.mjs
    let import_path = if code.contains("../../dependencies/") || code.contains("../../assets/") {
        "../../dependencies/acorn-icon.mjs"
    } else if code.contains("../dependencies/") || code.contains("../assets/") {
        "../dependencies/acorn-icon.mjs"
    } else {
        "./acorn-icon.mjs"
    };

    let import_stmt = format!("import \"{import_path}\";\n");

    // Don't add if already present
    if code.contains(&import_stmt) || code.contains("acorn-icon.mjs") {
        return code.to_string();
    }

    // Insert after the last import statement
    static LAST_IMPORT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?m)^import\s[^\n]*;\n").unwrap()
    });

    if let Some(m) = LAST_IMPORT.find_iter(code).last() {
        let insert_pos = m.end();
        let mut result = String::with_capacity(code.len() + import_stmt.len());
        result.push_str(&code[..insert_pos]);
        result.push_str(&import_stmt);
        result.push_str(&code[insert_pos..]);
        result
    } else {
        // No imports found, add at the top
        format!("{import_stmt}{code}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform_with_replacements(
        code: &str,
        replacements: &HashMap<String, String>,
    ) -> String {
        let ftl_map = FtlMap::new();
        let fluent_fallbacks = HashMap::new();
        transform_from_string(code, replacements, None, &ftl_map, &fluent_fallbacks).unwrap()
    }

    #[test]
    fn test_guard_custom_element_definitions() {
        let code = r#"customElements.define("moz-button", MozButton);"#;
        let result = guard_custom_element_definitions(code);
        assert!(result.contains("if (!customElements.get(\"moz-button\"))"));
        assert!(result.contains("customElements.define(\"moz-button\", MozButton)"));
    }

    #[test]
    fn test_guard_no_match() {
        let code = "const x = 42;";
        let result = guard_custom_element_definitions(code);
        assert_eq!(result, code);
    }

    #[test]
    fn test_chrome_url_replacement_in_js() {
        let mut map = HashMap::new();
        map.insert(
            "chrome://global/skin/icon.svg".to_string(),
            "../assets/icon.svg".to_string(),
        );
        let code = r#"const url = "chrome://global/skin/icon.svg";"#;
        let result = transform_with_replacements(code, &map);
        assert!(result.contains("../assets/icon.svg"));
        assert!(!result.contains("chrome://global/skin/icon.svg"));
    }

    #[test]
    fn test_import_rewrite() {
        let mut map = HashMap::new();
        map.insert(
            "chrome://global/content/lib.mjs".to_string(),
            "../../dependencies/lib.mjs".to_string(),
        );
        let code = r#"import { foo } from "chrome://global/content/lib.mjs";"#;
        let result = transform_with_replacements(code, &map);
        assert!(result.contains("../../dependencies/lib.mjs"));
    }

    #[test]
    fn test_remove_moz_xul_element_optional_chain() {
        let code = r#"import { LitElement } from "../../dependencies/lit.mjs";
window.MozXULElement?.insertFTLIfNeeded("toolkit/global/mozButton.ftl");
const x = 1;
"#;
        let result = remove_moz_xul_element_calls(code);
        assert!(!result.contains("MozXULElement"));
        assert!(!result.contains("insertFTLIfNeeded"));
        assert!(result.contains("const x = 1;"));
    }

    #[test]
    fn test_remove_moz_xul_element_dot_access() {
        let code = r#"import { LitElement } from "../../dependencies/lit.mjs";
MozXULElement.insertFTLIfNeeded("toolkit/global/mozFoo.ftl");
"#;
        let result = remove_moz_xul_element_calls(code);
        assert!(!result.contains("MozXULElement"));
    }

    #[test]
    fn test_remove_moz_xul_element_no_match() {
        let code = r#"const x = 42;"#;
        let result = remove_moz_xul_element_calls(code);
        assert_eq!(result, code);
    }

    #[test]
    fn test_add_fluent_fallbacks_static() {
        let mut ftl_map = FtlMap::new();
        let mut attrs = HashMap::new();
        attrs.insert("title".to_string(), "More options".to_string());
        attrs.insert("aria-label".to_string(), "More Options".to_string());
        ftl_map.insert("moz-button-more-options".to_string(), attrs);

        let code = r#"html`<button data-l10n-id="moz-button-more-options"></button>`"#;
        let result = add_fluent_fallbacks(code, &ftl_map, &HashMap::new());
        assert!(result.contains("data-l10n-id=\"moz-button-more-options\""));
        assert!(result.contains("aria-label=\"More Options\""));
        assert!(result.contains("title=\"More options\""));
    }

    #[test]
    fn test_add_fluent_fallbacks_with_l10n_attrs() {
        let mut ftl_map = FtlMap::new();
        let mut attrs = HashMap::new();
        attrs.insert("accesskey".to_string(), "o".to_string());
        attrs.insert("label".to_string(), "Browse…".to_string());
        ftl_map.insert("choose-folder-button".to_string(), attrs);

        let code = r#"html`<button data-l10n-id="choose-folder-button" data-l10n-attrs="accesskey"></button>`"#;
        let result = add_fluent_fallbacks(code, &ftl_map, &HashMap::new());
        assert!(result.contains("accesskey=\"o\""));
        assert!(!result.contains("label=\"Browse")); // filtered by data-l10n-attrs
        assert!(!result.contains("data-l10n-attrs")); // removed
    }

    #[test]
    fn test_guard_document_l10n_calls_with_ftl_value() {
        let mut ftl_map = FtlMap::new();
        let mut attrs = HashMap::new();
        attrs.insert(".value".to_string(), "Learn more".to_string());
        ftl_map.insert("my-link-text".to_string(), attrs);

        let code = r#"    document.l10n.setAttributes(this, "my-link-text");
    document.l10n.translateFragment(this);
"#;
        let result = guard_document_l10n_calls(code, &ftl_map);
        assert!(result.contains("if (!this.textContent)"));
        assert!(result.contains("Learn more"));
        assert!(result.contains("if (document.l10n) { document.l10n.setAttributes(this, \"my-link-text\"); }"));
        assert!(result.contains("if (document.l10n) { document.l10n.translateFragment(this); }"));
    }

    #[test]
    fn test_guard_document_l10n_calls_no_value() {
        let mut ftl_map = FtlMap::new();
        let mut attrs = HashMap::new();
        attrs.insert("title".to_string(), "Some title".to_string());
        ftl_map.insert("my-id".to_string(), attrs);

        let code = r#"    document.l10n.setAttributes(this, "my-id");
"#;
        let result = guard_document_l10n_calls(code, &ftl_map);
        // No textContent fallback since there's no .value
        assert!(!result.contains("textContent"));
        assert!(result.contains("if (document.l10n) { document.l10n.setAttributes(this, \"my-id\"); }"));
    }

    #[test]
    fn test_find_template_literal_end_simple() {
        let code = "hello world`rest";
        assert_eq!(find_template_literal_end(code, 0), Some(11));
    }

    #[test]
    fn test_find_template_literal_end_with_expression() {
        let code = "hello ${name} world`rest";
        assert_eq!(find_template_literal_end(code, 0), Some(19));
    }
}
