//! Transforms Firefox `.stories.md` documentation into Storybook `.mdx` format,
//! converting `<story>` blocks into JSX `<Canvas>` components and replacing `chrome://` URLs.

use std::collections::HashMap;
use std::path::PathBuf;

/// Converts a `.stories.md` file (Firefox Storybook markdown format) into
/// Storybook-compatible `.stories.mdx`.
///
/// Transformations applied:
/// 1. Chrome:// URLs replaced using the provided map
/// 2. ` ```html story` / ` ```js story` code blocks → raw JSX (rendered as live components)
/// 3. MDX header prepended with `<Meta>` component
pub fn transform_stories_md(
    content: &str,
    title: &str,
    chrome_url_map: &HashMap<String, PathBuf>,
) -> String {
    // Step 1: Replace chrome:// URLs
    let mut content = content.to_string();
    for (chrome_url, dist_path) in chrome_url_map {
        let replacement = format!("/{}", dist_path.display());
        content = content.replace(chrome_url, &replacement);
    }

    // Step 2: Replace ```html story / ```js story blocks with raw JSX
    content = replace_story_blocks(&content);

    // Step 3: Prepend MDX header
    let header = format!(
        r#"import {{ Meta }} from "@storybook/addon-docs/blocks";

<Meta title="{title}" />

"#
    );

    format!("{header}{content}")
}

/// Replaces ` ```html story` and ` ```js story` fenced code blocks with their
/// raw inner content (rendered as live JSX in MDX).
fn replace_story_blocks(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut lines = content.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```html story") || trimmed.starts_with("```js story") {
            // Extract indentation of the opening fence
            let fence_indent = line.len() - line.trim_start().len();

            // Collect inner lines until closing ```
            let mut inner = Vec::new();
            for inner_line in lines.by_ref() {
                let inner_trimmed = inner_line.trim();
                if inner_trimmed == "```" {
                    break;
                }
                inner.push(inner_line);
            }

            // Strip common leading whitespace from inner content
            let min_indent = inner
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0);

            // Use the larger of fence_indent and min_indent for stripping
            let strip = min_indent.max(fence_indent);

            for inner_line in &inner {
                if inner_line.trim().is_empty() {
                    result.push('\n');
                } else {
                    let safe_strip = strip.min(inner_line.len());
                    result.push_str(&inner_line[safe_strip..]);
                    result.push('\n');
                }
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

/// Computes a Storybook title for a global docs file.
///
/// `README.lit-guide.stories.md` → `"Docs/Lit Guide"`
pub fn title_for_global_doc(filename: &str) -> String {
    let name = filename
        .strip_prefix("README.")
        .unwrap_or(filename)
        .strip_suffix(".stories.md")
        .unwrap_or(filename);

    let words: Vec<String> = name
        .split(|c: char| c == '-' || c == '_' || c == '.')
        .filter(|w| !w.is_empty())
        .map(capitalize)
        .collect();

    format!("Docs/{}", words.join(" "))
}

/// Computes a Storybook title for a per-component README.
///
/// Component folder `moz-button` → `"UI Widgets/Button/README"`
/// Component folder `moz-box-group` → `"UI Widgets/Box Group/README"`
pub fn title_for_component_readme(component_folder: &str) -> String {
    let stripped = component_folder
        .strip_prefix("moz-")
        .unwrap_or(component_folder);

    let words: Vec<String> = stripped
        .split('-')
        .filter(|w| !w.is_empty())
        .map(capitalize)
        .collect();

    format!("UI Widgets/{}/README", words.join(" "))
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_for_global_doc() {
        assert_eq!(
            title_for_global_doc("README.lit-guide.stories.md"),
            "Docs/Lit Guide"
        );
        assert_eq!(
            title_for_global_doc("README.other-widgets.stories.md"),
            "Docs/Other Widgets"
        );
        assert_eq!(
            title_for_global_doc("README.typography.stories.md"),
            "Docs/Typography"
        );
        assert_eq!(
            title_for_global_doc("README.xul-and-html.stories.md"),
            "Docs/Xul And Html"
        );
    }

    #[test]
    fn test_title_for_component_readme() {
        assert_eq!(
            title_for_component_readme("moz-button"),
            "UI Widgets/Button/README"
        );
        assert_eq!(
            title_for_component_readme("moz-box-group"),
            "UI Widgets/Box Group/README"
        );
        assert_eq!(
            title_for_component_readme("moz-button-group"),
            "UI Widgets/Button Group/README"
        );
    }

    #[test]
    fn test_story_block_replacement() {
        let input = r#"# Title

Some text.

```html story
<moz-button label="Click me"></moz-button>
```

More text.
"#;
        let result = replace_story_blocks(input);
        assert!(result.contains("<moz-button label=\"Click me\"></moz-button>"));
        assert!(!result.contains("```html story"));
        assert!(result.contains("# Title"));
        assert!(result.contains("More text."));
    }

    #[test]
    fn test_story_block_indented() {
        let input = "      <td>\n        ```html story\n          <h1>Hello</h1>\n        ```\n      </td>\n";
        let result = replace_story_blocks(input);
        assert!(result.contains("<h1>Hello</h1>"));
        assert!(!result.contains("```html story"));
    }

    #[test]
    fn test_chrome_url_replacement() {
        let mut map = HashMap::new();
        map.insert(
            "chrome://global/skin/icons/more.svg".to_string(),
            PathBuf::from("assets/more.svg"),
        );

        let content = r#"<moz-button iconsrc="chrome://global/skin/icons/more.svg"></moz-button>"#;
        let result = transform_stories_md(content, "Test/Page", &map);
        assert!(result.contains(r#"iconsrc="/assets/more.svg""#));
        assert!(!result.contains("chrome://"));
    }

    #[test]
    fn test_mdx_header() {
        let map = HashMap::new();
        let result = transform_stories_md("# Hello", "Docs/My Page", &map);
        assert!(result.starts_with("import { Meta }"));
        assert!(result.contains(r#"<Meta title="Docs/My Page" />"#));
        assert!(result.contains("# Hello"));
    }
}
