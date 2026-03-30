use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// NodeId — newtype for type safety
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    #[allow(dead_code)]
    pub const ROOT: Self = Self(0);

    #[inline]
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// JsonValue — the value part of each node
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(Arc<str>),
    Array(Vec<NodeId>),
    Object(Vec<(Arc<str>, NodeId)>),
}

impl JsonValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "bool",
            JsonValue::Number(_) => "number",
            JsonValue::String(_) => "string",
            JsonValue::Array(_) => "array",
            JsonValue::Object(_) => "object",
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(self, JsonValue::Array(_) | JsonValue::Object(_))
    }

    pub fn child_count(&self) -> usize {
        match self {
            JsonValue::Array(children) => children.len(),
            JsonValue::Object(entries) => entries.len(),
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// JsonNode — a single node in the arena
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct JsonNode {
    pub parent: Option<NodeId>,
    pub value: JsonValue,
    pub depth: u16,
}

// ---------------------------------------------------------------------------
// DocumentMetadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DocumentMetadata {
    pub source_path: Option<PathBuf>,
    pub source_size: u64,
    pub parse_time: Duration,
    pub total_nodes: usize,
    pub max_depth: u16,
}

// ---------------------------------------------------------------------------
// JsonDocument — immutable arena-based JSON tree
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct JsonDocument {
    nodes: Vec<JsonNode>,
    root: NodeId,
    metadata: DocumentMetadata,
}

impl JsonDocument {
    /// Construct from pre-built parts. Used by the lazy parser.
    pub fn from_raw_parts(nodes: Vec<JsonNode>, root: NodeId, metadata: DocumentMetadata) -> Self {
        Self {
            nodes,
            root,
            metadata,
        }
    }

    #[inline]
    pub fn node(&self, id: NodeId) -> &JsonNode {
        &self.nodes[id.index()]
    }

    #[inline]
    pub fn root(&self) -> NodeId {
        self.root
    }

    #[inline]
    pub fn metadata(&self) -> &DocumentMetadata {
        &self.metadata
    }

    #[inline]
    #[cfg(test)]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Return the chain of node IDs from root to the given node (inclusive).
    pub fn ancestors_of(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = vec![id];
        let mut current = id;
        while let Some(parent) = self.node(current).parent {
            chain.push(parent);
            current = parent;
        }
        chain.reverse();
        chain
    }

    /// Build the full JSON path string for a node (e.g. `$.users[0].name`).
    pub fn path_of(&self, id: NodeId) -> String {
        let mut segments = Vec::new();
        let mut current = id;

        while let Some(parent_id) = self.node(current).parent {
            let parent = self.node(parent_id);
            match &parent.value {
                JsonValue::Array(children) => {
                    if let Some(idx) = children.iter().position(|&c| c == current) {
                        segments.push(format!("[{}]", idx));
                    }
                }
                JsonValue::Object(entries) => {
                    if let Some((key, _)) = entries.iter().find(|(_, c)| *c == current) {
                        if key.chars().all(|c| c.is_alphanumeric() || c == '_') && !key.is_empty() {
                            segments.push(format!(".{}", key));
                        } else {
                            segments.push(format!("[\"{}\"]", key));
                        }
                    }
                }
                _ => {}
            }
            current = parent_id;
        }

        segments.reverse();
        let mut path = String::from("$");
        for seg in segments {
            path.push_str(&seg);
        }
        path
    }
}

// ---------------------------------------------------------------------------
// Builder — constructs a JsonDocument from a serde_json::Value
// ---------------------------------------------------------------------------

pub struct DocumentBuilder {
    nodes: Vec<JsonNode>,
    max_depth: u16,
}

impl DocumentBuilder {
    pub fn from_serde_value(
        value: serde_json::Value,
        source_path: Option<PathBuf>,
        source_size: u64,
        parse_time: Duration,
    ) -> JsonDocument {
        let mut builder = DocumentBuilder {
            nodes: Vec::new(),
            max_depth: 0,
        };

        let root = builder.build_node(&value, None, 0);
        let total_nodes = builder.nodes.len();

        JsonDocument {
            nodes: builder.nodes,
            root,
            metadata: DocumentMetadata {
                source_path,
                source_size,
                parse_time,
                total_nodes,
                max_depth: builder.max_depth,
            },
        }
    }

    fn allocate(&mut self, node: JsonNode) -> NodeId {
        let len = self.nodes.len();
        assert!(
            len < u32::MAX as usize,
            "document exceeds maximum node count (4 billion)"
        );
        let id = NodeId(len as u32);
        self.nodes.push(node);
        id
    }

    /// Build the arena iteratively from a serde_json::Value tree.
    /// Uses a work-stack + ID-stack to avoid stack overflow on deeply nested JSON.
    fn build_node(
        &mut self,
        root_value: &serde_json::Value,
        root_parent: Option<NodeId>,
        root_depth: u16,
    ) -> NodeId {
        enum Work<'a> {
            /// Visit a value: allocate its node, push children onto work stack.
            Visit {
                value: &'a serde_json::Value,
                parent: Option<NodeId>,
                depth: u16,
            },
            /// Collect the last `count` IDs from `id_stack` as array children.
            BuildArray { id: NodeId, count: usize },
            /// Collect the last `count` IDs from `id_stack` as object entries.
            BuildObject { id: NodeId, keys: Vec<Arc<str>> },
        }

        let mut work = vec![Work::Visit {
            value: root_value,
            parent: root_parent,
            depth: root_depth,
        }];
        let mut id_stack: Vec<NodeId> = Vec::new();

        while let Some(item) = work.pop() {
            match item {
                Work::Visit { value, parent, depth } => {
                    self.max_depth = self.max_depth.max(depth);
                    let child_depth = depth.saturating_add(1);

                    match value {
                        serde_json::Value::Null => {
                            let id = self.allocate(JsonNode { parent, value: JsonValue::Null, depth });
                            id_stack.push(id);
                        }
                        serde_json::Value::Bool(b) => {
                            let id = self.allocate(JsonNode { parent, value: JsonValue::Bool(*b), depth });
                            id_stack.push(id);
                        }
                        serde_json::Value::Number(n) => {
                            let id = self.allocate(JsonNode { parent, value: JsonValue::Number(n.clone()), depth });
                            id_stack.push(id);
                        }
                        serde_json::Value::String(s) => {
                            let id = self.allocate(JsonNode { parent, value: JsonValue::String(Arc::from(s.as_str())), depth });
                            id_stack.push(id);
                        }
                        serde_json::Value::Array(items) => {
                            let id = self.allocate(JsonNode { parent, value: JsonValue::Array(Vec::new()), depth });
                            work.push(Work::BuildArray { id, count: items.len() });
                            for item in items.iter().rev() {
                                work.push(Work::Visit { value: item, parent: Some(id), depth: child_depth });
                            }
                        }
                        serde_json::Value::Object(map) => {
                            let id = self.allocate(JsonNode { parent, value: JsonValue::Object(Vec::new()), depth });
                            let keys: Vec<Arc<str>> = map.keys().map(|k| Arc::from(k.as_str())).collect();
                            work.push(Work::BuildObject { id, keys });
                            for val in map.values().rev() {
                                work.push(Work::Visit { value: val, parent: Some(id), depth: child_depth });
                            }
                        }
                    }
                }
                Work::BuildArray { id, count } => {
                    let start = id_stack.len() - count;
                    let children: Vec<NodeId> = id_stack.drain(start..).collect();
                    self.nodes[id.index()].value = JsonValue::Array(children);
                    id_stack.push(id);
                }
                Work::BuildObject { id, keys } => {
                    let start = id_stack.len() - keys.len();
                    let child_ids = id_stack.drain(start..);
                    let entries: Vec<(Arc<str>, NodeId)> = keys.into_iter().zip(child_ids).collect();
                    self.nodes[id.index()].value = JsonValue::Object(entries);
                    id_stack.push(id);
                }
            }
        }

        id_stack.pop().expect("build_node produced no nodes")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> JsonDocument {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "name": "Alice",
                "age": 30,
                "active": true,
                "address": null,
                "scores": [100, 95, 87],
                "meta": {"role": "admin"}
            }"#,
        )
        .unwrap();

        DocumentBuilder::from_serde_value(json, None, 0, Duration::ZERO)
    }

    #[test]
    fn root_is_object() {
        let doc = sample_doc();
        assert!(doc.node(doc.root()).value.is_container());
        assert_eq!(doc.node(doc.root()).value.child_count(), 6);
    }

    #[test]
    fn node_count_is_correct() {
        let doc = sample_doc();
        // root(1) + 6 values (name,age,active,address,scores,meta)
        // + 3 array items + 1 meta-role value = 11
        // Note: keys are stored in parent entries, not as separate nodes.
        assert_eq!(doc.node_count(), 11);
    }

    #[test]
    fn max_depth_is_correct() {
        let doc = sample_doc();
        assert_eq!(doc.metadata().max_depth, 2);
    }

    #[test]
    fn path_of_root() {
        let doc = sample_doc();
        assert_eq!(doc.path_of(doc.root()), "$");
    }

    #[test]
    fn path_of_nested_value() {
        let doc = sample_doc();
        // Find "scores" array, then its first child
        let root = doc.node(doc.root());
        if let JsonValue::Object(entries) = &root.value {
            let (_, scores_id) = entries
                .iter()
                .find(|(k, _)| k.as_ref() == "scores")
                .unwrap();
            let scores_node = doc.node(*scores_id);
            if let JsonValue::Array(items) = &scores_node.value {
                let path = doc.path_of(items[0]);
                assert_eq!(path, "$.scores[0]");
            } else {
                panic!("expected array");
            }
        } else {
            panic!("expected object");
        }
    }

    #[test]
    fn ancestors_of_root() {
        let doc = sample_doc();
        let ancestors = doc.ancestors_of(doc.root());
        assert_eq!(ancestors, vec![doc.root()]);
    }

    #[test]
    fn ancestors_of_nested() {
        let doc = sample_doc();
        let root = doc.node(doc.root());
        if let JsonValue::Object(entries) = &root.value {
            let (_, scores_id) = entries
                .iter()
                .find(|(k, _)| k.as_ref() == "scores")
                .unwrap();
            let scores_node = doc.node(*scores_id);
            if let JsonValue::Array(items) = &scores_node.value {
                let ancestors = doc.ancestors_of(items[0]);
                assert_eq!(ancestors.len(), 3); // root, scores, scores[0]
                assert_eq!(ancestors[0], doc.root());
                assert_eq!(ancestors[1], *scores_id);
                assert_eq!(ancestors[2], items[0]);
            } else {
                panic!("expected array");
            }
        } else {
            panic!("expected object");
        }
    }

    #[test]
    fn type_names() {
        assert_eq!(JsonValue::Null.type_name(), "null");
        assert_eq!(JsonValue::Bool(true).type_name(), "bool");
        assert_eq!(JsonValue::String(Arc::from("x")).type_name(), "string");
    }

    #[test]
    fn node_id_display() {
        assert_eq!(format!("{}", NodeId(42)), "#42");
    }
}
