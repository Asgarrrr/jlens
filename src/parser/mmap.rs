use std::path::Path;
use std::time::Instant;

use memmap2::Mmap;

use crate::model::node::{DocumentBuilder, JsonDocument};
use crate::parser::ParseError;

/// Parse a JSON file using memory-mapped I/O.
/// The OS manages paging — only accessed pages are loaded into physical memory.
/// Best for files between 50 MB and 500 MB.
pub fn parse(path: &Path) -> Result<JsonDocument, ParseError> {
    let file = std::fs::File::open(path)?;
    let source_size = file.metadata()?.len();

    // Safety: the file must not be modified while mapped.
    // For a read-only viewer this is acceptable.
    let mmap = unsafe { Mmap::map(&file)? };

    let start = Instant::now();
    let value: serde_json::Value = serde_json::from_slice(&mmap)?;
    let parse_time = start.elapsed();

    let doc =
        DocumentBuilder::from_serde_value(value, Some(path.to_path_buf()), source_size, parse_time);

    Ok(doc)
}
