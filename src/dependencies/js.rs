//! Extracts import dependencies and FTL references from JavaScript files using the
//! `oxc` parser's AST visitor pattern. Also scrapes `chrome://` URLs from html`` templates.

use std::path::Path;
use std::sync::LazyLock;

use oxc::{
    allocator::Allocator,
    ast::ast::{ImportDeclaration, StringLiteral, TemplateElement},
    ast_visit::Visit,
    parser::{Parser, ParserReturn},
    span::SourceType,
};
use regex::Regex;

use crate::errors::{Error, Result};

static LINK_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<link[^>]*rel\s*=\s*[\"']stylesheet[\"'][^>]*href\s*=\s*[\"']([^\"']+)[\"'][^>]*/?>"#,
    )
    .unwrap()
});

static URL_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:src|href|iconsrc)\s*=\s*[\"']([^\"']+\.[a-zA-Z0-9]+)[\"']"#).unwrap()
});

/// Collected dependencies from a JS file: import paths and Fluent `.ftl` references.
pub struct JsDependencies {
    pub imports: Vec<String>,
    pub ftl_references: Vec<String>,
}

/// Reads a JS/MJS file and extracts its import dependencies and FTL references.
pub fn dependencies_from_file(source_path: &Path) -> Result<JsDependencies> {
    let source_text = std::fs::read_to_string(source_path)?;
    let source_type = SourceType::from_path(source_path).unwrap();
    dependencies_from_string(&source_text, source_type)
}

/// Parses JS source text with `oxc` and walks the AST to collect import declarations,
/// `chrome://`/`resource://` string literals, FTL file references, and URLs from
/// `html``\`` template literals (link hrefs, img srcs, iconsrc attrs).
pub fn dependencies_from_string(
    source_text: &str,
    source_type: SourceType,
) -> Result<JsDependencies> {
    // Memory arena where AST nodes are allocated.
    let allocator = Allocator::default();

    let ParserReturn {
        program,
        errors: parser_errors,
        panicked,
        ..
    } = Parser::new(&allocator, source_text, source_type).parse();

    if panicked {
        return Err(Error::JsPanicParse);
    }

    if !parser_errors.is_empty() {
        let error_messages: Vec<String> =
            parser_errors.iter().map(|e| format!("{:?}", e)).collect();
        return Err(Error::JsParse {
            message: format!("Parser errors: {}", error_messages.join(", ")),
        });
    }

    let mut visitor = DependencyVisitor::new();
    visitor.visit_program(&program);

    let imports: Vec<String> = visitor
        .dependencies
        .into_iter()
        .filter(|dep| !dep.is_empty())
        .collect();

    Ok(JsDependencies {
        imports,
        ftl_references: visitor.ftl_references,
    })
}

struct DependencyVisitor {
    dependencies: Vec<String>,
    ftl_references: Vec<String>,
}

impl DependencyVisitor {
    fn new() -> Self {
        Self {
            dependencies: Vec::new(),
            ftl_references: Vec::new(),
        }
    }

    fn extract_string_literal(&mut self, literal: &StringLiteral) {
        self.dependencies.push(literal.value.to_string());
    }

    fn extract_css_links_from_html(&mut self, html_content: &str) {
        for captures in LINK_TAG_RE.captures_iter(html_content) {
            if let Some(href_match) = captures.get(1) {
                let href = href_match.as_str().trim();
                if !href.is_empty() {
                    self.dependencies.push(href.to_string());
                }
            }
        }
    }

    fn extract_any_link_from_html(&mut self, html_content: &str) {
        for captures in URL_ATTR_RE.captures_iter(html_content) {
            if let Some(url_match) = captures.get(1) {
                let url = url_match.as_str().trim();
                // Only allow relative paths or chrome:// or resource://
                if (url.starts_with("chrome://") || url.starts_with("resource://"))
                    || (!url.starts_with("http://")
                        && !url.starts_with("https://")
                        && !url.starts_with("www."))
                {
                    self.dependencies.push(url.to_string());
                }
            }
        }
    }
}

impl<'a> Visit<'a> for DependencyVisitor {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        self.extract_string_literal(&decl.source);
    }

    fn visit_template_element(&mut self, element: &TemplateElement<'a>) {
        // If the template element contains HTML, extract CSS links
        let value = &element.value;
        self.extract_css_links_from_html(&value.raw);
        self.extract_any_link_from_html(&value.raw);
    }

    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        if it.value.starts_with("chrome://") || it.value.starts_with("resource://") {
            self.extract_string_literal(it);
        }
        // Detect FTL file references (from insertFTLIfNeeded calls)
        if it.value.ends_with(".ftl") && it.value.contains('/') {
            self.ftl_references.push(it.value.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc::span::SourceType;

    fn parse(source: &str) -> JsDependencies {
        let st = SourceType::default().with_module(true);
        dependencies_from_string(source, st).unwrap()
    }

    #[test]
    fn test_import_extraction() {
        let deps = parse(r#"import "./foo.mjs"; import "../bar.css";"#);
        assert_eq!(deps.imports, vec!["./foo.mjs", "../bar.css"]);
    }

    #[test]
    fn test_ftl_references() {
        let deps = parse(r#"MozXULElement.insertFTLIfNeeded("toolkit/global/mozButton.ftl");"#);
        assert_eq!(deps.ftl_references, vec!["toolkit/global/mozButton.ftl"]);
        assert!(deps.imports.is_empty());
    }

    #[test]
    fn test_css_link_in_template() {
        let deps = parse(
            r#"const t = html`<link rel="stylesheet" href="chrome://global/skin/design.css" />`;"#,
        );
        assert!(
            deps.imports
                .contains(&"chrome://global/skin/design.css".to_string())
        );
    }

    #[test]
    fn test_img_src_in_template() {
        let deps = parse(r#"const t = html`<img src="chrome://global/skin/icon.svg" />`;"#);
        assert!(
            deps.imports
                .contains(&"chrome://global/skin/icon.svg".to_string())
        );
    }

    #[test]
    fn test_http_urls_filtered_from_templates() {
        let deps = parse(r#"const t = html`<a href="https://example.com/page.html">link</a>`;"#);
        assert!(
            !deps.imports.iter().any(|d| d.contains("example.com")),
            "HTTP URLs should be filtered out"
        );
    }

    #[test]
    fn test_chrome_string_literal() {
        let deps = parse(r#"const url = "chrome://global/skin/foo.mjs";"#);
        assert_eq!(deps.imports, vec!["chrome://global/skin/foo.mjs"]);
    }
}
