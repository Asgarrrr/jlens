use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::diff::{DiffNode, DiffResult, DiffStats, DiffStatus};
use crate::model::node::NodeId;
use crate::theme::Theme;
use crate::util::ScrollState;
use crate::views::{StatusInfo, View, ViewAction};

// ---------------------------------------------------------------------------
// Flattened row for rendering
// ---------------------------------------------------------------------------

struct FlatRow {
    /// Index into the DiffResult tree — we store a path to the node
    /// as a sequence of child indices from the root.
    node_path: Vec<usize>,
    status: DiffStatus,
    key: Option<String>,
    array_index: Option<usize>,
    left: Option<serde_json::Value>,
    right: Option<serde_json::Value>,
    depth: u16,
    is_container: bool,
    is_expanded: bool,
}

// ---------------------------------------------------------------------------
// DiffView
// ---------------------------------------------------------------------------

pub struct DiffView {
    result: DiffResult,
    rows: Vec<FlatRow>,
    /// Set of row indices (in `rows`) that are currently expanded.
    expanded: HashSet<usize>,
    scroll: ScrollState,
}

impl DiffView {
    pub fn new(result: DiffResult) -> Self {
        let mut view = Self {
            result,
            rows: Vec::new(),
            expanded: HashSet::new(),
            scroll: ScrollState::new(),
        };
        view.rebuild_rows();
        view
    }

    pub fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.rows.len());
    }

    pub fn stats(&self) -> &DiffStats {
        &self.result.stats
    }

    // -----------------------------------------------------------------------
    // Iterative DFS flattening
    // -----------------------------------------------------------------------

    fn rebuild_rows(&mut self) {
        self.rows.clear();

        // Stack frames: (node reference path from root, child index within parent)
        struct Frame {
            node_path: Vec<usize>,
            key: Option<String>,
            array_index: Option<usize>,
        }

        // Seed with root
        let root_expandable = !self.result.root.children.is_empty();
        // The root row gets index 0 after push.
        // We use an iterative stack that also carries the parent row index so we
        // can look up expanded state correctly.

        // Encode stack as: (path_to_node_from_root_children_indices, key, array_index)
        // but the root node itself has path = [] (empty = root).
        let mut stack: Vec<Frame> = vec![Frame {
            node_path: vec![],
            key: self.result.root.key.clone(),
            array_index: self.result.root.array_index,
        }];

        while let Some(frame) = stack.pop() {
            let node = self.node_at(&frame.node_path);
            let row_idx = self.rows.len();
            let is_container = !node.children.is_empty();
            let is_expanded = self.expanded.contains(&row_idx);

            // Extract all data from the node before the mutable borrow on self.rows.
            let status = node.status;
            let left = node.left.clone();
            let right = node.right.clone();
            let depth = node.depth;

            // Collect child frames if expanded (while node borrow is still active).
            let child_frames: Vec<Frame> = if is_container && is_expanded {
                (0..node.children.len())
                    .rev()
                    .map(|ci| {
                        let mut child_path = frame.node_path.clone();
                        child_path.push(ci);
                        Frame {
                            node_path: child_path,
                            key: node.children[ci].key.clone(),
                            array_index: node.children[ci].array_index,
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            // `node` borrow is now released.

            self.rows.push(FlatRow {
                node_path: frame.node_path,
                status,
                key: frame.key,
                array_index: frame.array_index,
                left,
                right,
                depth,
                is_container,
                is_expanded,
            });

            stack.extend(child_frames);
        }

        // Clamp scroll after rebuild
        let total = self.rows.len();
        self.scroll.clamp(total.max(1));
        _ = root_expandable;
    }

    fn node_at(&self, path: &[usize]) -> &DiffNode {
        let mut node = &self.result.root;
        for &idx in path {
            node = &node.children[idx];
        }
        node
    }

    // -----------------------------------------------------------------------
    // Navigation helpers
    // -----------------------------------------------------------------------

    fn toggle_expand(&mut self) {
        if let Some(row) = self.rows.get(self.scroll.selected) {
            if row.is_container {
                let idx = self.scroll.selected;
                if self.expanded.contains(&idx) {
                    self.expanded.remove(&idx);
                } else {
                    self.expanded.insert(idx);
                }
                self.rebuild_rows();
            }
        }
    }

    fn expand(&mut self) {
        if let Some(row) = self.rows.get(self.scroll.selected) {
            if row.is_container && !row.is_expanded {
                self.expanded.insert(self.scroll.selected);
                self.rebuild_rows();
            }
        }
    }

    fn collapse(&mut self) {
        let selected = self.scroll.selected;
        if let Some(row) = self.rows.get(selected) {
            if row.is_expanded {
                self.expanded.remove(&selected);
                self.rebuild_rows();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Row rendering
    // -----------------------------------------------------------------------

    fn render_row(&self, row: &FlatRow, is_selected: bool, theme: &Theme) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Status prefix + indentation
        let indent = "  ".repeat(row.depth as usize);
        let (prefix, style) = match row.status {
            DiffStatus::Added => ("+", theme.diff_added),
            DiffStatus::Removed => ("-", theme.diff_removed),
            DiffStatus::Modified => ("~", theme.diff_modified),
            DiffStatus::Unchanged => (" ", Style::new().fg(theme.fg)),
        };

        spans.push(Span::styled(
            format!("{}{} ", prefix, indent),
            style,
        ));

        // Expand/collapse icon for containers
        if row.is_container {
            let icon = if row.is_expanded { "▼ " } else { "▶ " };
            spans.push(Span::styled(icon.to_string(), Style::new().fg(theme.fg)));
        }

        // Key label
        if let Some(ref key) = row.key {
            spans.push(Span::styled(
                format!("\"{}\"", key),
                theme.key,
            ));
            spans.push(Span::styled(
                ": ".to_string(),
                Style::new().fg(theme.fg_dim),
            ));
        }

        // Array index label
        if let Some(idx) = row.array_index {
            spans.push(Span::styled(
                format!("[{}] ", idx),
                Style::new().fg(theme.fg_dim),
            ));
        }

        // Value display
        if row.is_container {
            // Show container summary
            let node = self.node_at(&row.node_path);
            let child_count = node.children.len();
            match (&row.left, &row.right) {
                (Some(serde_json::Value::Object(_)), _) | (_, Some(serde_json::Value::Object(_))) => {
                    spans.push(Span::styled("{".to_string(), theme.bracket));
                    if !row.is_expanded {
                        spans.push(Span::styled(
                            format!("{} keys", child_count),
                            Style::new().fg(theme.fg_dim),
                        ));
                        spans.push(Span::styled("}".to_string(), theme.bracket));
                    }
                }
                _ => {
                    spans.push(Span::styled("[".to_string(), theme.bracket));
                    if !row.is_expanded {
                        spans.push(Span::styled(
                            format!("{} items", child_count),
                            Style::new().fg(theme.fg_dim),
                        ));
                        spans.push(Span::styled("]".to_string(), theme.bracket));
                    }
                }
            }
        } else {
            match row.status {
                DiffStatus::Unchanged => {
                    if let Some(ref v) = row.right {
                        spans.push(Span::styled(value_display(v), Style::new().fg(theme.fg)));
                    }
                }
                DiffStatus::Added => {
                    if let Some(ref v) = row.right {
                        spans.push(Span::styled(value_display(v), theme.diff_added));
                    }
                }
                DiffStatus::Removed => {
                    if let Some(ref v) = row.left {
                        spans.push(Span::styled(value_display(v), theme.diff_removed));
                    }
                }
                DiffStatus::Modified => {
                    if let Some(ref lv) = row.left {
                        spans.push(Span::styled(value_display(lv), theme.diff_removed));
                    }
                    spans.push(Span::styled(
                        " → ".to_string(),
                        Style::new().fg(theme.fg_dim),
                    ));
                    if let Some(ref rv) = row.right {
                        spans.push(Span::styled(value_display(rv), theme.diff_added));
                    }
                }
            }
        }

        if is_selected {
            for span in &mut spans {
                span.style = span.style.bg(theme.selection_bg);
            }
            Line::from(spans).style(Style::new().bg(theme.selection_bg).fg(theme.selection_fg))
        } else {
            Line::from(spans)
        }
    }
}

// ---------------------------------------------------------------------------
// View trait
// ---------------------------------------------------------------------------

impl View for DiffView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = area.height as usize;
        if self.rows.is_empty() {
            let msg = Line::from(Span::styled(
                "No differences",
                Style::new().fg(theme.fg_dim),
            ));
            frame.render_widget(ratatui::widgets::Paragraph::new(msg), area);
            return;
        }

        let start = self.scroll.offset;
        let end = (start + height).min(self.rows.len());

        let lines: Vec<Line<'static>> = (start..end)
            .map(|i| {
                let row = &self.rows[i];
                self.render_row(row, i == self.scroll.selected, theme)
            })
            .collect();

        let paragraph = ratatui::widgets::Paragraph::new(lines)
            .style(Style::new().bg(theme.bg));
        frame.render_widget(paragraph, area);

        if self.rows.len() > height {
            crate::ui::render_scrollbar(frame, area, self.rows.len(), self.scroll.offset, theme);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        let total = self.rows.len();
        match (key.modifiers, key.code) {
            // Navigation
            (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                self.scroll.move_up();
            }
            (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                self.scroll.move_down(total);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
                self.scroll.page_up(2);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
                self.scroll.page_down(total, 2);
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.scroll.go_top();
            }
            (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                self.scroll.go_bottom(total);
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

            // Quit
            (KeyModifiers::NONE, KeyCode::Char('q')) => return ViewAction::Quit,
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return ViewAction::Quit,

            // Help
            (KeyModifiers::NONE, KeyCode::Char('?')) => return ViewAction::ToggleHelp,

            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        let stats = &self.result.stats;
        let extra = format!(
            "+{} -{} ~{}",
            stats.added, stats.removed, stats.modified
        );
        StatusInfo {
            cursor_path: format!("diff ({}/{})", self.scroll.selected + 1, self.rows.len()),
            extra: Some(extra),
        }
    }

    fn search_highlights(&self) -> &[NodeId] {
        &[]
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        let target = self.scroll.offset + row_in_viewport;
        if target < self.rows.len() {
            self.scroll.selected = target;
            self.scroll.ensure_visible();
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn value_display(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => {
            if s.chars().count() > 80 {
                format!("\"{}...\"", crate::util::truncate_chars(s, 77))
            } else {
                format!("\"{}\"", s)
            }
        }
        serde_json::Value::Array(a) => format!("[{} items]", a.len()),
        serde_json::Value::Object(m) => format!("{{{} keys}}", m.len()),
    }
}
