use crate::parser::ParseError;

// ---------------------------------------------------------------------------
// Byte-level JSON scanner
//
// Skips over complete JSON values in a byte slice without allocating or
// parsing. Used by the lazy/shallow parser to record byte ranges for deep
// containers that are only parsed on demand.
// ---------------------------------------------------------------------------

/// Skip ASCII whitespace starting at `offset`, returning the position of the
/// first non-whitespace byte (or `bytes.len()` if none).
#[inline]
pub fn skip_whitespace(bytes: &[u8], mut offset: usize) -> usize {
    while offset < bytes.len() {
        match bytes[offset] {
            b' ' | b'\t' | b'\n' | b'\r' => offset += 1,
            _ => break,
        }
    }
    offset
}

/// Skip a complete JSON value starting at (or after leading whitespace from)
/// `offset`. Returns the position immediately after the value's last byte.
///
/// On success the byte range `offset..return_value` (after stripping leading
/// whitespace) encompasses exactly one JSON value.
pub fn skip_value(bytes: &[u8], offset: usize) -> Result<usize, ParseError> {
    let i = skip_whitespace(bytes, offset);
    if i >= bytes.len() {
        return Err(scan_error(i, "unexpected end of input"));
    }

    match bytes[i] {
        b'{' | b'[' => skip_container(bytes, i),
        b'"' => skip_string(bytes, i),
        b't' => expect_literal(bytes, i, b"true"),
        b'f' => expect_literal(bytes, i, b"false"),
        b'n' => expect_literal(bytes, i, b"null"),
        b'-' | b'0'..=b'9' => skip_number(bytes, i),
        ch => Err(scan_error(
            i,
            &format!("unexpected byte 0x{:02X} ({:?})", ch, ch as char),
        )),
    }
}

/// Find the start position of the next value (skips whitespace) and return it.
/// Returns `Err` if the end of input is reached.
#[allow(dead_code)]
pub fn find_value_start(bytes: &[u8], offset: usize) -> Result<usize, ParseError> {
    let i = skip_whitespace(bytes, offset);
    if i >= bytes.len() {
        Err(scan_error(i, "unexpected end of input"))
    } else {
        Ok(i)
    }
}

// ---------------------------------------------------------------------------
// Container scanner — matches `{`…`}` and `[`…`]`
// ---------------------------------------------------------------------------

fn skip_container(bytes: &[u8], start: usize) -> Result<usize, ParseError> {
    debug_assert!(bytes[start] == b'{' || bytes[start] == b'[');

    let close = if bytes[start] == b'{' { b'}' } else { b']' };
    let mut depth: u32 = 1;
    let mut i = start + 1;

    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'"' => {
                // Skip over string contents (handles escapes)
                i = skip_string_body(bytes, i + 1)?;
            }
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            ch if ch == close || ch == b'}' || ch == b']' => {
                if ch == close && depth == 1 {
                    return Ok(i + 1);
                }
                // Any closing bracket/brace decrements depth for its opener.
                // In well-formed JSON the types always match, so we just track
                // nesting depth generically.
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }

    Err(scan_error(start, "unterminated container"))
}

// ---------------------------------------------------------------------------
// String scanner — `"`…`"`
// ---------------------------------------------------------------------------

/// Skip a complete JSON string starting at the opening `"`.
/// Returns the position immediately after the closing `"`.
fn skip_string(bytes: &[u8], start: usize) -> Result<usize, ParseError> {
    debug_assert!(bytes[start] == b'"');
    skip_string_body(bytes, start + 1)
}

/// Skip the body of a string (after the opening `"`), returning the position
/// immediately after the closing `"`.
fn skip_string_body(bytes: &[u8], mut i: usize) -> Result<usize, ParseError> {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2, // skip escape + next byte
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err(scan_error(i, "unterminated string"))
}

// ---------------------------------------------------------------------------
// Number scanner
// ---------------------------------------------------------------------------

fn skip_number(bytes: &[u8], start: usize) -> Result<usize, ParseError> {
    let mut i = start;

    // Optional leading minus
    if i < bytes.len() && bytes[i] == b'-' {
        i += 1;
    }

    // Integer part
    if i >= bytes.len() {
        return Err(scan_error(start, "unterminated number"));
    }
    if bytes[i] == b'0' {
        i += 1;
    } else if bytes[i].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return Err(scan_error(i, "expected digit in number"));
    }

    // Fractional part
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        if i >= bytes.len() || !bytes[i].is_ascii_digit() {
            return Err(scan_error(i, "expected digit after decimal point"));
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }

    // Exponent part
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        if i >= bytes.len() || !bytes[i].is_ascii_digit() {
            return Err(scan_error(i, "expected digit in exponent"));
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }

    Ok(i)
}

// ---------------------------------------------------------------------------
// Literal scanner (`true`, `false`, `null`)
// ---------------------------------------------------------------------------

fn expect_literal(bytes: &[u8], start: usize, literal: &[u8]) -> Result<usize, ParseError> {
    let end = start + literal.len();
    if end > bytes.len() || &bytes[start..end] != literal {
        Err(scan_error(
            start,
            &format!("expected {:?}", std::str::from_utf8(literal).unwrap()),
        ))
    } else {
        Ok(end)
    }
}

// ---------------------------------------------------------------------------
// Key extraction (for the shallow parser)
// ---------------------------------------------------------------------------

/// Extract an object key string starting at offset (which must point to `"`).
/// Returns `(key_string, position_after_key)`.
pub fn extract_key(bytes: &[u8], offset: usize) -> Result<(String, usize), ParseError> {
    if offset >= bytes.len() || bytes[offset] != b'"' {
        return Err(scan_error(offset, "expected '\"' at start of key"));
    }
    let end = skip_string(bytes, offset)?;
    // Parse the key with serde_json to handle escapes correctly.
    let key: String = serde_json::from_slice(&bytes[offset..end])
        .map_err(|_| scan_error(offset, "invalid string"))?;
    Ok((key, end))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scan_error(offset: usize, message: &str) -> ParseError {
    // Approximate line/column from offset (costly but only on error path).
    ParseError::Syntax {
        line: 0,
        column: offset,
        message: format!("scanner: {} at byte offset {}", message, offset),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_null() {
        assert_eq!(skip_value(b"null", 0).unwrap(), 4);
        assert_eq!(skip_value(b"  null  ", 0).unwrap(), 6);
    }

    #[test]
    fn skip_booleans() {
        assert_eq!(skip_value(b"true", 0).unwrap(), 4);
        assert_eq!(skip_value(b"false", 0).unwrap(), 5);
    }

    #[test]
    fn skip_integers() {
        assert_eq!(skip_value(b"42", 0).unwrap(), 2);
        assert_eq!(skip_value(b"-7", 0).unwrap(), 2);
        assert_eq!(skip_value(b"0", 0).unwrap(), 1);
        assert_eq!(skip_value(b"12345", 0).unwrap(), 5);
    }

    #[test]
    fn skip_floats() {
        assert_eq!(skip_value(b"3.14", 0).unwrap(), 4);
        assert_eq!(skip_value(b"-0.5", 0).unwrap(), 4);
        assert_eq!(skip_value(b"1e10", 0).unwrap(), 4);
        assert_eq!(skip_value(b"1.5E-3", 0).unwrap(), 6);
    }

    #[test]
    fn skip_simple_string() {
        assert_eq!(skip_value(br#""hello""#, 0).unwrap(), 7);
    }

    #[test]
    fn skip_string_with_escapes() {
        // "he\"llo"
        assert_eq!(skip_value(br#""he\"llo""#, 0).unwrap(), 9);
        // "back\\slash"
        assert_eq!(skip_value(br#""back\\slash""#, 0).unwrap(), 13);
    }

    #[test]
    fn skip_empty_object() {
        assert_eq!(skip_value(b"{}", 0).unwrap(), 2);
    }

    #[test]
    fn skip_empty_array() {
        assert_eq!(skip_value(b"[]", 0).unwrap(), 2);
    }

    #[test]
    fn skip_simple_object() {
        let json = br#"{"a": 1, "b": "two"}"#;
        assert_eq!(skip_value(json, 0).unwrap(), json.len());
    }

    #[test]
    fn skip_nested_containers() {
        let json = br#"{"a": [1, {"b": [2, 3]}, 4], "c": {}}"#;
        assert_eq!(skip_value(json, 0).unwrap(), json.len());
    }

    #[test]
    fn skip_array_of_arrays() {
        let json = br#"[[1, 2], [3, [4, 5]]]"#;
        assert_eq!(skip_value(json, 0).unwrap(), json.len());
    }

    #[test]
    fn skip_with_offset() {
        //          0123456789...
        let json = b"   [1, 2]  ";
        assert_eq!(skip_value(json, 0).unwrap(), 9);
        assert_eq!(skip_value(json, 3).unwrap(), 9);
    }

    #[test]
    fn skip_string_with_braces() {
        // String containing braces should not confuse the container scanner.
        let json = br#"{"key": "value with { and [ and ] and }"}"#;
        assert_eq!(skip_value(json, 0).unwrap(), json.len());
    }

    #[test]
    fn extract_simple_key() {
        let (key, end) = extract_key(br#""name""#, 0).unwrap();
        assert_eq!(key, "name");
        assert_eq!(end, 6);
    }

    #[test]
    fn extract_key_with_escape() {
        let (key, end) = extract_key(br#""na\"me""#, 0).unwrap();
        assert_eq!(key, "na\"me");
        assert_eq!(end, 8);
    }

    #[test]
    fn error_on_unterminated_string() {
        assert!(skip_value(br#""hello"#, 0).is_err());
    }

    #[test]
    fn error_on_unterminated_object() {
        assert!(skip_value(b"{\"a\": 1", 0).is_err());
    }

    #[test]
    fn error_on_empty_input() {
        assert!(skip_value(b"", 0).is_err());
        assert!(skip_value(b"   ", 0).is_err());
    }

    #[test]
    fn skip_value_at_offset_in_array() {
        // Parse second element in: [42, "hello", true]
        let json = b"[42, \"hello\", true]";
        // After "[42, " → offset 5
        let end = skip_value(json, 5).unwrap();
        assert_eq!(end, 12); // past "hello"
        // After "hello", " → offset 14
        let end2 = skip_value(json, 14).unwrap();
        assert_eq!(end2, 18); // past true
    }

    #[test]
    fn whitespace_skipping() {
        assert_eq!(skip_whitespace(b"  \t\n  x", 0), 6);
        assert_eq!(skip_whitespace(b"abc", 0), 0);
        assert_eq!(skip_whitespace(b"", 0), 0);
    }
}
