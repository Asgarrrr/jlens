pub mod algo;
pub mod view;

/// Status of a node in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Unchanged,
    Added,
    Removed,
    Modified,
}

/// A node in the diff tree.
pub struct DiffNode {
    pub status: DiffStatus,
    pub key: Option<String>,
    pub array_index: Option<usize>,
    pub left: Option<serde_json::Value>,
    pub right: Option<serde_json::Value>,
    pub children: Vec<DiffNode>,
    pub depth: u16,
}

pub struct DiffResult {
    pub root: DiffNode,
    pub stats: DiffStats,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub unchanged: usize,
}
