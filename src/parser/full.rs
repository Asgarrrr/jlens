use std::path::Path;
use std::time::Instant;

use crate::model::node::{DocumentBuilder, JsonDocument};
use crate::parser::ParseError;

/// Parse a JSON file entirely into memory.
/// Best for files under ~50 MB.
///
/// When compiled with the `simd` feature, uses `simd-json` for faster parsing
/// on x86-64 and aarch64 targets. Otherwise falls back to `serde_json`.
pub fn parse(path: &Path) -> Result<JsonDocument, ParseError> {
    let source_size = std::fs::metadata(path)?.len();

    let start = Instant::now();

    #[cfg(feature = "simd")]
    let value: serde_json::Value = {
        let mut bytes = std::fs::read(path)?;
        simd_json::serde::from_slice(&mut bytes)?
    };

    #[cfg(not(feature = "simd"))]
    let value: serde_json::Value = {
        use std::fs::File;
        use std::io::BufReader;
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(256 * 1024, file);
        serde_json::from_reader(reader)?
    };

    let parse_time = start.elapsed();

    let doc =
        DocumentBuilder::from_serde_value(value, Some(path.to_path_buf()), source_size, parse_time);

    Ok(doc)
}
