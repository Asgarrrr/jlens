use std::collections::HashSet;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;
use crate::views::raw::rebuild_serde_value;
use crate::views::{StatusInfo, View, ViewAction};

type ChildEntry = (Option<Arc<str>>, Option<usize>, NodeId, bool);

// ---------------------------------------------------------------------------
// Tree connectors (Unicode box-drawing)
// ---------------------------------------------------------------------------

const CONNECTOR_PIPE: &str = "│  ";
const CONNECTOR_TEE: &str = "├─ ";
const CONNECTOR_ELBOW: &str = "└─ ";
const CONNECTOR_BLANK: &str = "   ";
const ICON_EXPANDED: &str = "▼ ";
const ICON_COLLAPSED: &str = "▶ ";

// ---------------------------------------------------------------------------
// FlattenedRow — one visible row in the tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FlattenedRow {
    pub node_id: NodeId,
    pub depth: u16,
    pub key: Option<Arc<str>>,
    pub array_index: Option<usize>,
    pub is_expandable: bool,
    pub is_expanded: bool,
    /// Bitmask: bit `i` is set when the ancestor at depth `i+1` has more
    /// siblings below this row (i.e. a vertical continuation line is needed).
    /// Max supported depth is 64, which is sufficient for real-world JSON.
    pub continuation: u64,
    pub is_last_sibling: bool,
}

// ---------------------------------------------------------------------------
// StackFrame — used by the iterative flatten / expand algorithms
// ---------------------------------------------------------------------------

struct FlattenFrame {
    id: NodeId,
    depth: u16,
    key: Option<Arc<str>>,
    array_index: Option<usize>,
    continuation: u64,
    is_last: bool,
}

// ---------------------------------------------------------------------------
// TreeView
// ---------------------------------------------------------------------------

pub struct TreeView {
    document: Arc<JsonDocument>,
    expanded: HashSet<NodeId>,
    selected: usize,
    scroll_offset: usize,
    visible_rows: Vec<FlattenedRow>,
    dirty: bool,
    search_matches: HashSet<NodeId>,
    /// The node that is the "active" search hit (n/N navigation target),
    /// rendered with `theme.search_current` to distinguish it from other matches.
    current_search_node: Option<NodeId>,
    viewport_height: usize,
    /// Node IDs that are lazy stubs with unparsed children.
    stub_ids: HashSet<NodeId>,
    /// Deferred action: a stub expansion was requested and needs app-level handling.
    pending_expand_stub: Option<NodeId>,
}

impl TreeView {
    pub fn new(document: Arc<JsonDocument>) -> Self {
        let mut expanded = HashSet::new();
        // Auto-expand root
        expanded.insert(document.root());

        let mut view = Self {
            document,
            expanded,
            selected: 0,
            scroll_offset: 0,
            visible_rows: Vec::new(),
            dirty: true,
            search_matches: HashSet::new(),
            current_search_node: None,
            viewport_height: 0,
            stub_ids: HashSet::new(),
            pending_expand_stub: None,
        };
        view.rebuild_visible_rows();
        view
    }

    // -----------------------------------------------------------------------
    // Flattening — converts tree + expanded set into a linear row list
    // -----------------------------------------------------------------------

    fn rebuild_visible_rows(&mut self) {
        self.visible_rows.clear();
        self.flatten_node_iterative();
        self.dirty = false;

        // Clamp selected index
        if !self.visible_rows.is_empty() {
            self.selected = self.selected.min(self.visible_rows.len() - 1);
        }
    }

    /// Iterative depth-first flattening using an explicit stack.
    /// Replaces the former recursive `flatten_node` to avoid stack overflow
    /// on deeply nested JSON and to eliminate per-recursion Vec clones.
    fn flatten_node_iterative(&mut self) {
        let root = self.document.root();

        let mut stack = vec![FlattenFrame {
            id: root,
            depth: 0,
            key: None,
            array_index: None,
            continuation: 0,
            is_last: true,
        }];

        while let Some(frame) = stack.pop() {
            let node = self.document.node(frame.id);
            let is_expandable = node.value.is_container();
            let is_expanded = self.expanded.contains(&frame.id);

            // Collect children BEFORE pushing the row, avoiding borrow issues.
            // We gather (key, array_index, child_id, is_last_child) tuples.
            let children: Vec<ChildEntry> =
                if is_expanded && is_expandable {
                    match &node.value {
                        JsonValue::Array(ids) => {
                            let len = ids.len();
                            ids.iter()
                                .enumerate()
                                .map(|(i, &cid)| (None, Some(i), cid, i == len - 1))
                                .collect()
                        }
                        JsonValue::Object(entries) => {
                            let len = entries.len();
                            entries
                                .iter()
                                .enumerate()
                                .map(|(i, (k, cid))| {
                                    (Some(k.clone()), None, *cid, i == len - 1)
                                })
                                .collect()
                        }
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                };

            self.visible_rows.push(FlattenedRow {
                node_id: frame.id,
                depth: frame.depth,
                key: frame.key,
                array_index: frame.array_index,
                is_expandable,
                is_expanded,
                continuation: frame.continuation,
                is_last_sibling: frame.is_last,
            });

            // Push children in reverse order so the first child is processed first.
            for (child_key, child_idx, child_id, is_last_child) in children.into_iter().rev() {
                // Build continuation bitmask for the child. Bit `depth` (0-indexed)
                // records whether the CURRENT node (the child's parent at this depth)
                // has more siblings after it — i.e. `!frame.is_last`.
                let child_cont = if !frame.is_last {
                    frame.continuation | (1u64 << frame.depth)
                } else {
                    // Parent is last at its level — clear the bit (already 0).
                    frame.continuation & !(1u64 << frame.depth)
                };

                stack.push(FlattenFrame {
                    id: child_id,
                    depth: frame.depth + 1,
                    key: child_key,
                    array_index: child_idx,
                    continuation: child_cont,
                    is_last: is_last_child,
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.ensure_visible();
    }

    fn move_down(&mut self) {
        if !self.visible_rows.is_empty() {
            self.selected = (self.selected + 1).min(self.visible_rows.len() - 1);
        }
        self.ensure_visible();
    }

    fn page_up(&mut self) {
        let jump = self.viewport_height.saturating_sub(2);
        self.selected = self.selected.saturating_sub(jump);
        self.ensure_visible();
    }

    fn page_down(&mut self) {
        if !self.visible_rows.is_empty() {
            let jump = self.viewport_height.saturating_sub(2);
            self.selected = (self.selected + jump).min(self.visible_rows.len() - 1);
        }
        self.ensure_visible();
    }

    fn go_top(&mut self) {
        self.selected = 0;
        self.ensure_visible();
    }

    fn go_bottom(&mut self) {
        if !self.visible_rows.is_empty() {
            self.selected = self.visible_rows.len() - 1;
        }
        self.ensure_visible();
    }

    fn toggle_expand(&mut self) {
        if let Some(row) = self.visible_rows.get(self.selected) {
            if row.is_expandable {
                let id = row.node_id;
                if self.expanded.contains(&id) {
                    self.expanded.remove(&id);
                    self.dirty = true;
                    self.rebuild_visible_rows();
                } else if self.stub_ids.contains(&id) {
                    // This is a lazy stub — signal to App for expansion.
                    self.pending_expand_stub = Some(id);
                } else {
                    self.expanded.insert(id);
                    self.dirty = true;
                    self.rebuild_visible_rows();
                }
            }
        }
    }

    fn expand(&mut self) {
        if let Some(row) = self.visible_rows.get(self.selected) {
            if row.is_expandable && !row.is_expanded {
                if self.stub_ids.contains(&row.node_id) {
                    self.pending_expand_stub = Some(row.node_id);
                } else {
                    self.expanded.insert(row.node_id);
                    self.dirty = true;
                    self.rebuild_visible_rows();
                }
            }
        }
    }

    fn collapse(&mut self) {
        if let Some(row) = self.visible_rows.get(self.selected) {
            if row.is_expanded {
                // Collapse current node
                self.expanded.remove(&row.node_id);
                self.dirty = true;
                self.rebuild_visible_rows();
            } else if let Some(parent) = self.document.node(row.node_id).parent {
                // Navigate to parent
                if let Some(pos) = self.visible_rows.iter().position(|r| r.node_id == parent) {
                    self.selected = pos;
                    self.ensure_visible();
                }
            }
        }
    }

    fn expand_all(&mut self) {
        self.expand_subtree_iterative(self.document.root());
        self.dirty = true;
        self.rebuild_visible_rows();
    }

    /// Iterative subtree expansion using an explicit stack.
    /// Replaces the former recursive `expand_subtree` to avoid stack overflow
    /// and eliminate Vec clones of child ID lists.
    fn expand_subtree_iterative(&mut self, root: NodeId) {
        let mut stack = vec![root];

        while let Some(id) = stack.pop() {
            let node = self.document.node(id);
            if !node.value.is_container() {
                continue;
            }
            self.expanded.insert(id);

            // Collect child IDs before mutating self.expanded on the next iteration.
            match &node.value {
                JsonValue::Array(children) => {
                    stack.extend(children.iter().copied());
                }
                JsonValue::Object(entries) => {
                    stack.extend(entries.iter().map(|(_, cid)| *cid));
                }
                _ => {}
            }
        }
    }

    fn collapse_all(&mut self) {
        self.expanded.clear();
        self.expanded.insert(self.document.root());
        self.selected = 0;
        self.dirty = true;
        self.rebuild_visible_rows();
    }

    fn ensure_visible(&mut self) {
        if self.viewport_height == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        if self.selected >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = self.selected - self.viewport_height + 1;
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    fn render_row(&self, row: &FlattenedRow, is_selected: bool, theme: &Theme) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Tree guide characters
        if row.depth > 0 {
            // Continuation lines for ancestors.
            // Ancestor at depth `i` (1-based) stores its continuation flag
            // in bit `(i - 1)` of the bitmask.
            for i in 1..row.depth {
                let has_continuation = (row.continuation >> (i - 1)) & 1 != 0;
                let connector = if has_continuation {
                    CONNECTOR_PIPE
                } else {
                    CONNECTOR_BLANK
                };
                spans.push(Span::styled(
                    connector.to_string(),
                    theme.tree_guide_style,
                ));
            }

            // This node's connector
            let connector = if row.is_last_sibling {
                CONNECTOR_ELBOW
            } else {
                CONNECTOR_TEE
            };
            spans.push(Span::styled(
                connector.to_string(),
                theme.tree_guide_style,
            ));
        }

        // Expand/collapse icon for containers
        if row.is_expandable {
            let icon = if row.is_expanded {
                ICON_EXPANDED
            } else {
                ICON_COLLAPSED
            };
            spans.push(Span::styled(
                icon.to_string(),
                theme.fg_style,
            ));
        }

        // Key (if in an object)
        if let Some(ref key) = row.key {
            spans.push(Span::styled(format!("\"{}\"", key), theme.key));
            spans.push(Span::styled(
                ": ".to_string(),
                theme.fg_dim_style,
            ));
        }

        // Array index (if in an array)
        if let Some(idx) = row.array_index {
            spans.push(Span::styled(
                format!("[{}] ", idx),
                theme.fg_dim_style,
            ));
        }

        // Value rendering
        let node = self.document.node(row.node_id);
        match &node.value {
            JsonValue::Null => {
                spans.push(Span::styled("null".to_string(), theme.null));
            }
            JsonValue::Bool(b) => {
                spans.push(Span::styled(b.to_string(), theme.boolean));
            }
            JsonValue::Number(n) => {
                spans.push(Span::styled(n.to_string(), theme.number));
            }
            JsonValue::String(s) => {
                let display = if s.chars().count() > 80 {
                    format!("\"{}...\"", crate::util::truncate_chars(s, 77))
                } else {
                    format!("\"{}\"", s)
                };
                spans.push(Span::styled(display, theme.string));
            }
            JsonValue::Array(_) => {
                let is_stub = self.stub_ids.contains(&row.node_id);
                spans.push(Span::styled("[".to_string(), theme.bracket));
                if !row.is_expanded {
                    let label = if is_stub {
                        "\u{2026}".to_string() // "…"
                    } else {
                        format!("{} items", node.value.child_count())
                    };
                    spans.push(Span::styled(label, theme.fg_dim_style));
                    spans.push(Span::styled("]".to_string(), theme.bracket));
                }
            }
            JsonValue::Object(_) => {
                let is_stub = self.stub_ids.contains(&row.node_id);
                spans.push(Span::styled("{".to_string(), theme.bracket));
                if !row.is_expanded {
                    let label = if is_stub {
                        "\u{2026}".to_string() // "…"
                    } else {
                        format!("{} keys", node.value.child_count())
                    };
                    spans.push(Span::styled(label, theme.fg_dim_style));
                    spans.push(Span::styled("}".to_string(), theme.bracket));
                }
            }
        }

        // Determine row highlight: selection > current search hit > any search match.
        let is_current_hit = self.current_search_node == Some(row.node_id);
        let is_search_match = self.search_matches.contains(&row.node_id);

        if is_selected {
            let sel_style = theme.selection_style;
            for span in &mut spans {
                span.style = span.style.bg(theme.selection_bg);
            }
            Line::from(spans).style(sel_style)
        } else if is_current_hit {
            let hit_bg = theme.search_current.bg.unwrap_or(theme.selection_bg);
            for span in &mut spans {
                span.style = span.style.bg(hit_bg);
            }
            Line::from(spans).style(theme.search_current)
        } else if is_search_match {
            let match_bg = theme.search_match.bg.unwrap_or(theme.selection_bg);
            for span in &mut spans {
                span.style = span.style.bg(match_bg);
            }
            Line::from(spans).style(theme.search_match)
        } else {
            Line::from(spans)
        }
    }
}

// ---------------------------------------------------------------------------
// View trait implementation
// ---------------------------------------------------------------------------

impl View for TreeView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = area.height as usize;
        if self.visible_rows.is_empty() {
            let empty = Line::from(Span::styled(
                "Empty document",
                theme.fg_dim_style,
            ));
            frame.render_widget(ratatui::widgets::Paragraph::new(empty), area);
            return;
        }

        let start = self.scroll_offset;
        let end = (start + height).min(self.visible_rows.len());

        let lines: Vec<Line<'static>> = (start..end)
            .map(|i| {
                let row = &self.visible_rows[i];
                let is_selected = i == self.selected;
                self.render_row(row, is_selected, theme)
            })
            .collect();

        let paragraph = ratatui::widgets::Paragraph::new(lines)
            .style(theme.bg_style);

        frame.render_widget(paragraph, area);

        if self.visible_rows.len() > height {
            crate::ui::render_scrollbar(
                frame,
                area,
                self.visible_rows.len(),
                self.scroll_offset,
                theme,
            );
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match (key.modifiers, key.code) {
            // Navigation
            (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                self.move_up();
            }
            (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                self.move_down();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
                self.page_up();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
                self.page_down();
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.go_top();
            }
            (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                self.go_bottom();
            }

            // Expand/collapse
            (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                self.toggle_expand();
            }
            (KeyModifiers::NONE, KeyCode::Right) | (KeyModifiers::NONE, KeyCode::Char('l')) => {
                self.expand();
            }
            (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h')) => {
                self.collapse();
            }

            // Expand/collapse all
            (KeyModifiers::NONE, KeyCode::Char('e')) => {
                self.expand_all();
            }
            (KeyModifiers::SHIFT, KeyCode::Char('E')) => {
                self.collapse_all();
            }

            // Copy value
            (KeyModifiers::NONE, KeyCode::Char('y')) => {
                if let Some(row) = self.visible_rows.get(self.selected) {
                    let value_str = node_to_clipboard_string(&self.document, row.node_id);
                    return ViewAction::CopyToClipboard(value_str);
                }
            }
            // Copy path
            (KeyModifiers::SHIFT, KeyCode::Char('Y')) => {
                if let Some(row) = self.visible_rows.get(self.selected) {
                    let path = self.document.path_of(row.node_id);
                    return ViewAction::CopyToClipboard(path);
                }
            }

            _ => {}
        }

        // Check if a stub expansion was requested.
        if let Some(stub_id) = self.pending_expand_stub.take() {
            return ViewAction::ExpandStub(stub_id);
        }

        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        if let Some(row) = self.visible_rows.get(self.selected) {
            let path = self.document.path_of(row.node_id);
            let node = self.document.node(row.node_id);
            let extra = match &node.value {
                JsonValue::Array(_) => Some(format!("array[{}]", node.value.child_count())),
                JsonValue::Object(_) => Some(format!("object{{{}}}", node.value.child_count())),
                _ => Some(node.value.type_name().to_string()),
            };
            StatusInfo {
                cursor_path: path,
                extra,
            }
        } else {
            StatusInfo {
                cursor_path: "$".to_string(),
                extra: None,
            }
        }
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height;
        self.ensure_visible();
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        let target = self.scroll_offset + row_in_viewport;
        if target < self.visible_rows.len() {
            self.selected = target;
            self.ensure_visible();
        }
    }
}

impl TreeView {
    #[allow(dead_code)]
    pub fn document(&self) -> &JsonDocument {
        &self.document
    }

    /// Navigate to a specific node, expanding all ancestors and scrolling to it.
    pub fn navigate_to_node(&mut self, target: NodeId) {
        // Expand all ancestors from root to target
        let mut ancestors = Vec::new();
        let mut current = target;
        while let Some(parent) = self.document.node(current).parent {
            ancestors.push(parent);
            current = parent;
        }

        for ancestor in ancestors {
            self.expanded.insert(ancestor);
        }

        self.dirty = true;
        self.rebuild_visible_rows();

        // Find the target in visible rows and select it
        if let Some(pos) = self.visible_rows.iter().position(|r| r.node_id == target) {
            self.selected = pos;
            self.ensure_visible();
        }
    }

    /// Replace the set of nodes matching the current search query.
    /// Clears `current_search_node` since the old active hit is no longer
    /// meaningful with a new match set.
    pub fn set_search_matches(&mut self, matches: HashSet<NodeId>) {
        self.search_matches = matches;
        self.current_search_node = None;
    }

    /// Set which match is the active navigation target (n/N).
    /// Rendered with `theme.search_current` to distinguish it from other hits.
    pub fn set_current_search_node(&mut self, id: Option<NodeId>) {
        self.current_search_node = id;
    }

    /// Returns the `NodeId` of the currently selected visible row, if any.
    pub fn selected_node_id(&self) -> Option<NodeId> {
        self.visible_rows.get(self.selected).map(|r| r.node_id)
    }

    /// Set the IDs of stub nodes (lazy-loaded containers with unparsed children).
    pub fn set_stub_ids(&mut self, stubs: HashSet<NodeId>) {
        self.stub_ids = stubs;
    }

    /// Replace the underlying document (after lazy expansion), mark a
    /// newly-expanded stub as expanded, and rebuild visible rows.
    pub fn update_document(&mut self, doc: Arc<JsonDocument>, expand_node: Option<NodeId>) {
        self.document = doc;
        if let Some(id) = expand_node {
            self.expanded.insert(id);
        }
        self.dirty = true;
        self.rebuild_visible_rows();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn node_to_clipboard_string(doc: &JsonDocument, id: NodeId) -> String {
    let node = doc.node(id);
    match &node.value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => format!("\"{}\"", s),
        JsonValue::Array(_) | JsonValue::Object(_) => {
            let value = rebuild_serde_value(doc, id);
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{...}".to_string())
        }
    }
}
