//! Unified error types for the transformation pipeline.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Glob pattern error: {0}")]
    Glob(#[from] glob::PatternError),
    #[error("Glob walk error: {0}")]
    GlobWalk(#[from] glob::GlobError),
    #[error("Failed to parse JavaScript: {message}")]
    JsParse { message: String },
    #[error("Failed to parse CSS: {message}")]
    CssParse { message: String },
    #[error("JavaScript parsing panicked")]
    JsPanicParse,
    #[error("Failed to transform CSS: {message}")]
    CssTransform { message: String },
    #[error("URL '{url}' not found in replacement map")]
    UrlNotFound { url: String },
    #[error("Failed to serialize CSS: {message}")]
    CssSerialize { message: String },
    #[error("Failed to extract dependencies: {message}")]
    DependencyExtract { message: String },
    #[error("{0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, Error>;
