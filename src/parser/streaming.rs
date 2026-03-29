use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use memmap2::Mmap;

use crate::model::lazy::LazyDocument;
use crate::parser::ParseError;

/// Parse a very large JSON file lazily using memory-mapped I/O.
/// Only the root structure and first-level children are parsed initially.
/// Deeper levels are parsed on-demand when the user expands nodes.
///
/// Best for files > 500 MB.
pub fn parse(path: &Path) -> Result<LazyDocument, ParseError> {
    let file = std::fs::File::open(path)?;
    let source_size = file.metadata()?.len();

    // Safety: the file must not be modified while mapped.
    // For a read-only viewer this is acceptable.
    let mmap = unsafe { Mmap::map(&file)? };
    let mmap = Arc::new(mmap);

    let doc = LazyDocument::from_mmap(
        mmap,
        Some(path.to_path_buf()),
        source_size,
        Instant::now(),
    )?;

    Ok(doc)
}
