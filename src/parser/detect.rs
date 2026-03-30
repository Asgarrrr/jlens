use std::path::Path;

use crate::parser::{ParseError, ParseOutcome};

/// Parse a JSON file. Always uses mmap + lazy shallow parse for consistent
/// behavior across all file sizes — from 1 KB to 1 TB.
///
/// Small files parse fully in the shallow pass (all content within depth/child limits).
/// Large files parse progressively, with deeper sections expanded on demand.
pub fn parse_ex(path: &Path) -> Result<ParseOutcome, ParseError> {
    Ok(ParseOutcome::Lazy(super::streaming::parse(path)?))
}
