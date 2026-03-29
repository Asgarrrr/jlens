use std::path::Path;

use crate::parser::{ParseError, ParseOutcome};

/// Size thresholds for parsing strategy selection.
const FULL_PARSE_LIMIT: u64 = 50 * 1024 * 1024; // 50 MB
const MMAP_FULL_LIMIT: u64 = 500 * 1024 * 1024; // 500 MB

/// Auto-detect the best parsing strategy based on file size and parse the file.
///
/// Strategies:
/// - Under 50 MB: full serde_json parse with BufReader
/// - 50-500 MB: memory-mapped file + full serde_json parse (zero-copy read)
/// - Over 500 MB: memory-mapped + lazy shallow parse (depth 0-1 only)
/// Like [`parse`] but preserves lazy-loading capabilities for large files.
pub fn parse_ex(path: &Path) -> Result<ParseOutcome, ParseError> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();

    if size <= FULL_PARSE_LIMIT {
        Ok(ParseOutcome::Full(super::full::parse(path)?))
    } else if size <= MMAP_FULL_LIMIT {
        Ok(ParseOutcome::Full(super::mmap::parse(path)?))
    } else {
        Ok(ParseOutcome::Lazy(super::streaming::parse(path)?))
    }
}
