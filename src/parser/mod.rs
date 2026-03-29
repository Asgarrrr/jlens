pub mod detect;
pub mod full;
pub mod mmap;
pub mod scan;
pub mod streaming;

use std::path::Path;

use thiserror::Error;

use crate::model::lazy::LazyDocument;
use crate::model::node::JsonDocument;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON syntax error at line {line}, column {column}: {message}")]
    Syntax {
        line: usize,
        column: usize,
        message: String,
    },
}

impl From<serde_json::Error> for ParseError {
    fn from(err: serde_json::Error) -> Self {
        Self::Syntax {
            line: err.line(),
            column: err.column(),
            message: err.to_string(),
        }
    }
}

#[cfg(feature = "simd")]
impl From<simd_json::Error> for ParseError {
    fn from(err: simd_json::Error) -> Self {
        Self::Syntax {
            line: 0,
            column: 0,
            message: err.to_string(),
        }
    }
}

/// Result of parsing: either a fully-parsed document, or a lazily-parsed one
/// with stubs that can be expanded on demand.
pub enum ParseOutcome {
    Full(JsonDocument),
    Lazy(LazyDocument),
}

/// Parse a JSON file from the given path, auto-detecting the best strategy.
pub fn parse_file(path: &Path) -> Result<JsonDocument, ParseError> {
    match parse_file_ex(path)? {
        ParseOutcome::Full(doc) => Ok(doc),
        ParseOutcome::Lazy(lazy) => Ok(lazy.into_document()),
    }
}

/// Like [`parse_file`] but preserves lazy-loading capabilities for large files.
pub fn parse_file_ex(path: &Path) -> Result<ParseOutcome, ParseError> {
    detect::parse_ex(path)
}
