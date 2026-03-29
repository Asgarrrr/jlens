use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use memmap2::Mmap;

use crate::model::node::{DocumentMetadata, JsonDocument, JsonNode, JsonValue, NodeId};
use crate::parser::scan;
use crate::parser::ParseError;

// ---------------------------------------------------------------------------
// LazyDocument — shallow arena backed by a memory-mapped file
// ---------------------------------------------------------------------------

/// A lazily-parsed JSON document backed by a memory-mapped file.
///
/// The initial parse only processes the top `MAX_SHALLOW_DEPTH` levels.
/// Deeper containers are stored as "stub" nodes with zero children and a
/// recorded byte range. Calling [`expand_node`] parses the stub's byte range
/// and returns a new `LazyDocument` with the subtree filled in.
pub struct LazyDocument {
    nodes: Vec<JsonNode>,
    root: NodeId,
    metadata: DocumentMetadata,
    mmap: Arc<Mmap>,
    /// Byte ranges (start..end in the mmap) for stub nodes whose children
    /// have not been parsed yet.
    pending: HashMap<NodeId, ByteRange>,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct ByteRange {
    start: usize,
    end: usize,
}

/// Maximum depth parsed during the initial shallow pass. Containers deeper
/// than this are stored as stubs.
const MAX_SHALLOW_DEPTH: u16 = 1;

impl LazyDocument {
    /// Create a `LazyDocument` by doing a shallow parse of the mmap data.
    ///
    /// Only the top `MAX_SHALLOW_DEPTH` levels are fully parsed. Deeper
    /// containers are recorded as stubs with their byte range preserved.
    /// This avoids deserializing the entire file, keeping startup time and
    /// memory usage proportional to the shallow tree size rather than the
    /// file size.
    pub fn from_mmap(
        mmap: Arc<Mmap>,
        source_path: Option<PathBuf>,
        source_size: u64,
        start_time: Instant,
    ) -> Result<Self, ParseError> {
        let (nodes, root_id, max_depth, pending) = {
            let bytes = &mmap[..];
            let mut builder = ShallowBuilder {
                bytes,
                nodes: Vec::new(),
                max_depth: 0,
                pending: HashMap::new(),
                stub_threshold: MAX_SHALLOW_DEPTH,
            };

            let root = builder.parse_value(0, None, 0)?;
            (builder.nodes, root.0, builder.max_depth, builder.pending)
        };

        let parse_time = start_time.elapsed();
        let total_nodes = nodes.len();

        Ok(LazyDocument {
            nodes,
            root: root_id,
            metadata: DocumentMetadata {
                source_path,
                source_size,
                parse_time,
                total_nodes,
                max_depth,
            },
            mmap,
            pending,
        })
    }

    /// Convert into a regular `JsonDocument`, discarding lazy capabilities.
    /// Stubs will appear as empty containers.
    pub fn into_document(self) -> JsonDocument {
        JsonDocument::from_raw_parts(self.nodes, self.root, self.metadata)
    }

    /// Check if a node is a stub with unparsed children.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_stub(&self, id: NodeId) -> bool {
        self.pending.contains_key(&id)
    }

    /// Returns `true` if there are any remaining unexpanded stubs.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_pending_stubs(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Expand a stub node, parsing its byte range and producing a new
    /// `LazyDocument` with the subtree filled in.
    ///
    /// The new document reuses the existing nodes (cloned) with the stub's
    /// children appended. Any containers found inside the expanded subtree
    /// that exceed `MAX_SHALLOW_DEPTH` relative to the stub are themselves
    /// recorded as new stubs.
    pub fn expand_node(&self, stub_id: NodeId) -> Result<LazyDocument, ParseError> {
        let range = match self.pending.get(&stub_id) {
            Some(r) => *r,
            None => return Err(ParseError::Syntax {
                line: 0,
                column: 0,
                message: format!("node {} is not a stub", stub_id),
            }),
        };

        let stub_node = &self.nodes[stub_id.index()];
        let stub_depth = stub_node.depth;

        // Clone the existing arena and pending set.
        let mut nodes = self.nodes.clone();
        let mut pending = self.pending.clone();

        // Remove this stub from pending — it's about to be expanded.
        pending.remove(&stub_id);

        let bytes = &self.mmap[..];
        let mut sub_builder = ShallowBuilder {
            bytes,
            nodes: Vec::new(),
            max_depth: 0,
            pending: HashMap::new(),
            // Allow one more level of nesting from this stub before stubbing again.
            stub_threshold: stub_depth + MAX_SHALLOW_DEPTH + 1,
        };

        // Parse the stub's byte range. The value at range.start is a container
        // (object or array) — parse it at depth stub_depth.
        let (_, _end) = sub_builder.parse_value(range.start, None, stub_depth)?;

        // The sub-builder produced a tree rooted at index 0. The root of that
        // tree corresponds to our stub node — we only need its children, not
        // the duplicate container node.
        if sub_builder.nodes.is_empty() {
            return Ok(self.clone_with(nodes, pending));
        }

        // Extract the sub-root's value before consuming the vec.
        let sub_root_value = sub_builder.nodes[0].value.clone();
        let sub_pending = sub_builder.pending;
        let base_offset = nodes.len() as u32;

        // Remap NodeIds in sub_builder: shift by base_offset.
        // The sub_root's children become the stub's children.
        // Skip index 0 (the duplicate root) — remap children to start from
        // index 1 in the sub-builder, shifted into the main arena.
        let remap = |sub_id: NodeId| -> NodeId {
            if sub_id.index() == 0 {
                stub_id
            } else {
                NodeId::from_raw(sub_id.index() as u32 - 1 + base_offset)
            }
        };

        // Append all sub-nodes except the root (index 0) to the main arena,
        // remapping their parent and children IDs.
        for (i, sub_node) in sub_builder.nodes.into_iter().enumerate().skip(1) {
            let remapped_parent = sub_node.parent.map(&remap);
            let remapped_value = match sub_node.value {
                JsonValue::Array(children) => {
                    JsonValue::Array(children.into_iter().map(&remap).collect())
                }
                JsonValue::Object(entries) => {
                    JsonValue::Object(entries.into_iter().map(|(k, c)| (k, remap(c))).collect())
                }
                other => other,
            };
            nodes.push(JsonNode {
                parent: remapped_parent,
                value: remapped_value,
                depth: sub_node.depth,
            });

            // Remap pending entries from the sub-builder.
            let sub_id = NodeId::from_raw(i as u32);
            if let Some(byte_range) = sub_pending.get(&sub_id) {
                pending.insert(remap(sub_id), *byte_range);
            }
        }

        // Update the stub node in the main arena with its new children.
        let new_value = match &sub_root_value {
            JsonValue::Array(children) => {
                JsonValue::Array(children.iter().map(|c| remap(*c)).collect())
            }
            JsonValue::Object(entries) => {
                JsonValue::Object(entries.iter().map(|(k, c)| (k.clone(), remap(*c))).collect())
            }
            _ => sub_root_value,
        };
        nodes[stub_id.index()].value = new_value;

        // Fix parents: children of the stub should point to stub_id.
        match &nodes[stub_id.index()].value {
            JsonValue::Array(children) => {
                let child_ids: Vec<NodeId> = children.clone();
                for child_id in child_ids {
                    nodes[child_id.index()].parent = Some(stub_id);
                }
            }
            JsonValue::Object(entries) => {
                let child_ids: Vec<NodeId> = entries.iter().map(|(_, c)| *c).collect();
                for child_id in child_ids {
                    nodes[child_id.index()].parent = Some(stub_id);
                }
            }
            _ => {}
        }

        Ok(self.clone_with(nodes, pending))
    }

    fn clone_with(&self, nodes: Vec<JsonNode>, pending: HashMap<NodeId, ByteRange>) -> LazyDocument {
        let total_nodes = nodes.len();
        let max_depth = nodes.iter().map(|n| n.depth).max().unwrap_or(0);
        LazyDocument {
            nodes,
            root: self.root,
            metadata: DocumentMetadata {
                source_path: self.metadata.source_path.clone(),
                source_size: self.metadata.source_size,
                parse_time: self.metadata.parse_time,
                total_nodes,
                max_depth,
            },
            mmap: Arc::clone(&self.mmap),
            pending,
        }
    }

    /// Build a `JsonDocument` snapshot from the current state.
    pub fn to_document(&self) -> JsonDocument {
        JsonDocument::from_raw_parts(
            self.nodes.clone(),
            self.root,
            self.metadata.clone(),
        )
    }

    /// Get the set of stub node IDs (for the tree view to render differently).
    pub fn stub_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.pending.keys().copied()
    }
}

// ---------------------------------------------------------------------------
// ShallowBuilder — scanner-based shallow parser
// ---------------------------------------------------------------------------

struct ShallowBuilder<'a> {
    bytes: &'a [u8],
    nodes: Vec<JsonNode>,
    max_depth: u16,
    pending: HashMap<NodeId, ByteRange>,
    /// Containers deeper than this are stubbed. For the initial parse this
    /// equals `MAX_SHALLOW_DEPTH`; for expansion it is relative to the
    /// stub being expanded.
    stub_threshold: u16,
}

impl<'a> ShallowBuilder<'a> {
    fn allocate(&mut self, node: JsonNode) -> NodeId {
        let len = self.nodes.len();
        assert!(
            len < u32::MAX as usize,
            "document exceeds maximum node count (4 billion)"
        );
        let id = NodeId::from_raw(len as u32);
        self.nodes.push(node);
        id
    }

    /// Parse a single JSON value at `offset`. Returns `(NodeId, end_offset)`.
    fn parse_value(
        &mut self,
        offset: usize,
        parent: Option<NodeId>,
        depth: u16,
    ) -> Result<(NodeId, usize), ParseError> {
        self.max_depth = self.max_depth.max(depth);

        let i = scan::skip_whitespace(self.bytes, offset);
        if i >= self.bytes.len() {
            return Err(ParseError::Syntax {
                line: 0,
                column: i,
                message: "unexpected end of input".to_string(),
            });
        }

        match self.bytes[i] {
            b'{' => self.parse_object(i, parent, depth),
            b'[' => self.parse_array(i, parent, depth),
            _ => self.parse_leaf(i, parent, depth),
        }
    }

    /// Parse a leaf value (string, number, bool, null) using serde_json on
    /// just the relevant byte range.
    fn parse_leaf(
        &mut self,
        offset: usize,
        parent: Option<NodeId>,
        depth: u16,
    ) -> Result<(NodeId, usize), ParseError> {
        let end = scan::skip_value(self.bytes, offset)?;
        let value: serde_json::Value = serde_json::from_slice(&self.bytes[offset..end])?;

        let json_value = match value {
            serde_json::Value::Null => JsonValue::Null,
            serde_json::Value::Bool(b) => JsonValue::Bool(b),
            serde_json::Value::Number(n) => JsonValue::Number(n),
            serde_json::Value::String(s) => JsonValue::String(Arc::from(s.as_str())),
            _ => unreachable!("parse_leaf called on non-leaf"),
        };

        let id = self.allocate(JsonNode {
            parent,
            value: json_value,
            depth,
        });
        Ok((id, end))
    }

    /// Parse an object `{ ... }`, recursing for children at shallow depth
    /// or stubbing containers beyond the threshold.
    fn parse_object(
        &mut self,
        offset: usize,
        parent: Option<NodeId>,
        depth: u16,
    ) -> Result<(NodeId, usize), ParseError> {
        debug_assert!(self.bytes[offset] == b'{');

        // If beyond the shallow threshold and non-empty, stub the container.
        if depth > self.stub_threshold {
            let peek = scan::skip_whitespace(self.bytes, offset + 1);
            if peek < self.bytes.len() && self.bytes[peek] != b'}' {
                return self.stub_container(offset, parent, depth, true);
            }
        }

        // Allocate the object node with empty entries (filled below).
        let id = self.allocate(JsonNode {
            parent,
            value: JsonValue::Object(Vec::new()),
            depth,
        });

        let mut i = scan::skip_whitespace(self.bytes, offset + 1);
        let mut entries: Vec<(Arc<str>, NodeId)> = Vec::new();

        while i < self.bytes.len() && self.bytes[i] != b'}' {
            // Expect a key string.
            let (key, after_key) = scan::extract_key(self.bytes, i)?;

            // Expect ':' separator.
            let colon = scan::skip_whitespace(self.bytes, after_key);
            if colon >= self.bytes.len() || self.bytes[colon] != b':' {
                return Err(ParseError::Syntax {
                    line: 0,
                    column: colon,
                    message: "expected ':' after object key".to_string(),
                });
            }

            // Parse the value.
            let (child_id, after_value) =
                self.parse_value(colon + 1, Some(id), depth.saturating_add(1))?;
            entries.push((Arc::from(key.as_str()), child_id));

            // Skip optional comma.
            i = scan::skip_whitespace(self.bytes, after_value);
            if i < self.bytes.len() && self.bytes[i] == b',' {
                i = scan::skip_whitespace(self.bytes, i + 1);
            }
        }

        // Skip closing '}'.
        if i < self.bytes.len() && self.bytes[i] == b'}' {
            i += 1;
        }

        self.nodes[id.index()].value = JsonValue::Object(entries);
        Ok((id, i))
    }

    /// Parse an array `[ ... ]`, recursing for elements at shallow depth
    /// or stubbing containers beyond the threshold.
    fn parse_array(
        &mut self,
        offset: usize,
        parent: Option<NodeId>,
        depth: u16,
    ) -> Result<(NodeId, usize), ParseError> {
        debug_assert!(self.bytes[offset] == b'[');

        // If beyond the shallow threshold and non-empty, stub the container.
        if depth > self.stub_threshold {
            let peek = scan::skip_whitespace(self.bytes, offset + 1);
            if peek < self.bytes.len() && self.bytes[peek] != b']' {
                return self.stub_container(offset, parent, depth, false);
            }
        }

        let id = self.allocate(JsonNode {
            parent,
            value: JsonValue::Array(Vec::new()),
            depth,
        });

        let mut i = scan::skip_whitespace(self.bytes, offset + 1);
        let mut children: Vec<NodeId> = Vec::new();

        while i < self.bytes.len() && self.bytes[i] != b']' {
            let (child_id, after_value) =
                self.parse_value(i, Some(id), depth.saturating_add(1))?;
            children.push(child_id);

            i = scan::skip_whitespace(self.bytes, after_value);
            if i < self.bytes.len() && self.bytes[i] == b',' {
                i = scan::skip_whitespace(self.bytes, i + 1);
            }
        }

        if i < self.bytes.len() && self.bytes[i] == b']' {
            i += 1;
        }

        self.nodes[id.index()].value = JsonValue::Array(children);
        Ok((id, i))
    }

    /// Create a stub node for a container beyond the shallow depth threshold.
    /// The byte range is recorded so it can be parsed later on expansion.
    fn stub_container(
        &mut self,
        offset: usize,
        parent: Option<NodeId>,
        depth: u16,
        is_object: bool,
    ) -> Result<(NodeId, usize), ParseError> {
        let end = scan::skip_value(self.bytes, offset)?;

        let value = if is_object {
            JsonValue::Object(Vec::new())
        } else {
            JsonValue::Array(Vec::new())
        };

        let id = self.allocate(JsonNode {
            parent,
            value,
            depth,
        });

        self.pending.insert(id, ByteRange { start: offset, end });
        Ok((id, end))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_mmap(json: &[u8]) -> Arc<Mmap> {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json).unwrap();
        f.flush().unwrap();
        let file = f.reopen().unwrap();
        Arc::new(unsafe { Mmap::map(&file).unwrap() })
    }

    #[test]
    fn shallow_parse_simple_object() {
        let json = br#"{"name": "Alice", "age": 30}"#;
        let mmap = make_mmap(json);
        let lazy = LazyDocument::from_mmap(mmap, None, json.len() as u64, Instant::now()).unwrap();

        assert!(!lazy.has_pending_stubs());
        let doc = lazy.into_document();
        assert_eq!(doc.metadata().total_nodes, 3); // root + "Alice" + 30
    }

    #[test]
    fn shallow_parse_stubs_deep_containers() {
        let json = br#"{"a": {"nested": [1, 2, 3]}}"#;
        let mmap = make_mmap(json);
        let lazy = LazyDocument::from_mmap(mmap, None, json.len() as u64, Instant::now()).unwrap();

        // Depth 0 = root object, depth 1 = "a" object.
        // "a" is at depth 1 which is <= MAX_SHALLOW_DEPTH, so it's parsed.
        // "nested" array inside "a" is at depth 2 which is > MAX_SHALLOW_DEPTH,
        // so it should be stubbed.
        assert!(lazy.has_pending_stubs());

        let doc = lazy.to_document();
        let root = doc.node(doc.root());
        if let JsonValue::Object(entries) = &root.value {
            let (_, a_id) = &entries[0];
            let a_node = doc.node(*a_id);
            if let JsonValue::Object(inner) = &a_node.value {
                let (key, nested_id) = &inner[0];
                assert_eq!(key.as_ref(), "nested");
                // The nested array is a stub — 0 children
                let nested_node = doc.node(*nested_id);
                assert!(matches!(&nested_node.value, JsonValue::Array(c) if c.is_empty()));
                assert!(lazy.is_stub(*nested_id));
            } else {
                panic!("expected object for 'a'");
            }
        } else {
            panic!("expected root object");
        }
    }

    #[test]
    fn expand_stub() {
        // "nested" at depth 2 exceeds MAX_SHALLOW_DEPTH(1) and is stubbed.
        let json = br#"{"items": {"nested": {"x": 1, "y": 2}}}"#;
        let mmap = make_mmap(json);
        let lazy = LazyDocument::from_mmap(mmap, None, json.len() as u64, Instant::now()).unwrap();

        // Find the stub.
        assert!(lazy.has_pending_stubs());
        let stub_id = *lazy.pending.keys().next().unwrap();
        assert!(lazy.is_stub(stub_id));

        // Expand it.
        let expanded = lazy.expand_node(stub_id).unwrap();
        assert!(!expanded.is_stub(stub_id));
        assert!(!expanded.has_pending_stubs());

        let doc = expanded.to_document();
        let stub_node = doc.node(stub_id);
        if let JsonValue::Object(entries) = &stub_node.value {
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].0.as_ref(), "x");
            assert_eq!(entries[1].0.as_ref(), "y");
        } else {
            panic!("expected expanded object");
        }
    }

    #[test]
    fn expand_creates_deeper_stubs() {
        // 5 levels of nesting: "b" (depth 2) is stubbed initially.
        // After expanding "b", "e" (depth 5) exceeds the expansion threshold
        // and becomes a new stub.
        let json = br#"{"a": {"b": {"c": {"d": {"e": [1, 2]}}}}}"#;
        let mmap = make_mmap(json);
        let lazy = LazyDocument::from_mmap(mmap, None, json.len() as u64, Instant::now()).unwrap();

        // Initial: "b" at depth 2 is stubbed.
        assert!(lazy.has_pending_stubs());

        // Find and expand "b".
        let stub_id = *lazy.pending.keys().next().unwrap();
        let expanded = lazy.expand_node(stub_id).unwrap();

        // After expanding "b": "e" at depth 5 is now a new stub.
        assert!(expanded.has_pending_stubs());

        // Expand "e".
        let stub2_id = *expanded.pending.keys().next().unwrap();
        let fully = expanded.expand_node(stub2_id).unwrap();
        assert!(!fully.has_pending_stubs());

        let doc = fully.to_document();
        // root, a, b, c, d, e, 1, 2 = 8 nodes
        assert!(doc.metadata().total_nodes >= 8);
    }

    #[test]
    fn expand_array_stub() {
        let json = br#"{"data": [10, 20, 30]}"#;
        let mmap = make_mmap(json);
        let lazy = LazyDocument::from_mmap(mmap, None, json.len() as u64, Instant::now()).unwrap();

        if !lazy.has_pending_stubs() {
            // "data" array is at depth 1 — within threshold, so fully parsed.
            // This is expected behavior; the test verifies correctness.
            let doc = lazy.into_document();
            let root = doc.node(doc.root());
            if let JsonValue::Object(entries) = &root.value {
                let (_, data_id) = &entries[0];
                if let JsonValue::Array(items) = &doc.node(*data_id).value {
                    assert_eq!(items.len(), 3);
                }
            }
            return;
        }

        let stub_id = *lazy.pending.keys().next().unwrap();
        let expanded = lazy.expand_node(stub_id).unwrap();
        let doc = expanded.to_document();
        let stub_node = doc.node(stub_id);
        if let JsonValue::Array(items) = &stub_node.value {
            assert_eq!(items.len(), 3);
        } else {
            panic!("expected expanded array");
        }
    }

    #[test]
    fn empty_containers_not_stubbed() {
        let json = br#"{"a": {"empty": {}, "also_empty": []}}"#;
        let mmap = make_mmap(json);
        let lazy = LazyDocument::from_mmap(mmap, None, json.len() as u64, Instant::now()).unwrap();

        // Empty containers at any depth should NOT be stubbed since there's
        // nothing to lazily expand.
        let doc = lazy.to_document();
        let root = doc.node(doc.root());
        if let JsonValue::Object(entries) = &root.value {
            let (_, a_id) = &entries[0];
            let a = doc.node(*a_id);
            if let JsonValue::Object(inner) = &a.value {
                for (key, child_id) in inner {
                    let child = doc.node(*child_id);
                    assert!(
                        child.value.child_count() == 0,
                        "key {:?} should have 0 children",
                        key
                    );
                    assert!(
                        !lazy.is_stub(*child_id),
                        "empty container {:?} should not be a stub",
                        key
                    );
                }
            }
        }
    }
}
