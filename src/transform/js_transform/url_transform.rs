//! Rewrites `import` declaration source strings (e.g. `chrome://` -> relative paths)
//! by traversing the AST with `oxc_traverse`. Appends `?url` for default CSS imports
//! so Vite returns the asset URL instead of processing the CSS.

use std::collections::HashMap;

use oxc::ast::ast::ImportDeclaration;
use oxc_traverse::{ReusableTraverseCtx, Traverse, TraverseCtx};

use crate::errors::{Error, Result};

/// Rewrites import declaration URLs by traversing the AST with `oxc_traverse`.
/// Errors if an import URL has no entry in the replacement map (except `lit.all.mjs`).
pub struct UrlTransformer<'a> {
    url_replacements: &'a HashMap<String, String>,
    error: Option<Error>,
}

impl<'a> UrlTransformer<'a> {
    pub fn new(url_replacements: &'a HashMap<String, String>) -> Self {
        Self {
            url_replacements,
            error: None,
        }
    }

    /// Traverses the program AST, rewriting all import source strings in-place.
    /// Returns `Err(UrlNotFound)` if any import URL is missing from the replacement map.
    pub fn build(
        &mut self,
        program: &mut oxc::ast::ast::Program<'a>,
        ctx: &mut ReusableTraverseCtx<'a, ()>,
    ) -> Result<()> {
        oxc_traverse::traverse_mut_with_ctx(self, program, ctx);
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        Ok(())
    }
}

impl<'a> Traverse<'a, ()> for UrlTransformer<'a> {
    fn enter_import_declaration(
        &mut self,
        node: &mut ImportDeclaration<'a>,
        ctx: &mut TraverseCtx<'a, ()>,
    ) {
        if self.error.is_some() {
            return;
        }

        let value = node.source.value.as_str();

        // ignore if value == "lit.all.mjs"
        if value == "lit.all.mjs" {
            return;
        }

        if let Some(replacement) = self.url_replacements.get(value) {
            // CSS files imported with a default specifier (e.g. `import styles from "...css"`)
            // need `?url` so Vite returns the asset URL as a string instead of trying to
            // process it as a CSS module.
            let has_default_import = node.specifiers.as_ref().is_some_and(|specs| {
                specs.iter().any(|s| {
                    matches!(
                        s,
                        oxc::ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(_)
                    )
                })
            });
            let final_replacement = if replacement.ends_with(".css") && has_default_import {
                format!("{replacement}?url")
            } else {
                replacement.clone()
            };
            node.source.value = ctx.ast.atom_from_strs_array([final_replacement.as_str()]);
        } else {
            self.error = Some(Error::UrlNotFound {
                url: value.to_string(),
            });
        }
    }
}
