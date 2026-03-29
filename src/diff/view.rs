use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::diff::{DiffNode, DiffResult, DiffStats, DiffStatus};
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
    /// Expanded nodes, identified by their stable path through the DiffResult
    /// tree (sequence of child indices from the root). Unlike row indices,
    /// these don't shift when the flattened row list is rebuilt.
    expanded: HashSet<Vec<usize>>,
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

    pub fn stats(&self) -> &DiffStats {
        &self.result.stats
    }

    // -----------------------------------------------------------------------
    // Iterative DFS flattening
    // -----------------------------------------------------------------------

    fn rebuild_rows(&mut self) {
        self.rows.clear();

        struct Frame {
            node_path: Vec<usize>,
            key: Option<String>,
            array_index: Option<usize>,
        }

        let mut stack: Vec<Frame> = vec![Frame {
            node_path: vec![],
            key: self.result.root.key.clone(),
            array_index: self.result.root.array_index,
        }];

        while let Some(frame) = stack.pop() {
            let node = self.node_at(&frame.node_path);
            let is_container = !node.children.is_empty();
            let is_expanded = self.expanded.contains(&frame.node_path);

            // Extract all data from the node before the mutable borrow on
            // self.rows (the push below). The `node` reference borrows
            // `self.result` immutably; `self.rows.push` borrows `self` mutably.
            let status = node.status;
            let left = node.left.clone();
            let right = node.right.clone();
            let depth = node.depth;

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

        let total = self.rows.len();
        self.scroll.clamp(total.max(1));
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
                let path = row.node_path.clone();
                if self.expanded.contains(&path) {
                    self.expanded.remove(&path);
                } else {
                    self.expanded.insert(path);
                }
                self.rebuild_rows();
            }
        }
    }

    fn expand(&mut self) {
        if let Some(row) = self.rows.get(self.scroll.selected) {
            if row.is_container && !row.is_expanded {
                self.expanded.insert(row.node_path.clone());
                self.rebuild_rows();
            }
        }
    }

    fn collapse(&mut self) {
        if let Some(row) = self.rows.get(self.scroll.selected) {
            if row.is_expanded {
                self.expanded.remove(&row.node_path);
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
            DiffStatus::Unchanged => (" ", theme.fg_style),
        };

        spans.push(Span::styled(
            format!("{}{} ", prefix, indent),
            style,
        ));

        // Expand/collapse icon for containers
        if row.is_container {
            let icon = if row.is_expanded { "▼ " } else { "▶ " };
            spans.push(Span::styled(icon.to_string(), theme.fg_style));
        }

        // Key label
        if let Some(ref key) = row.key {
            spans.push(Span::styled(
                format!("\"{}\"", key),
                theme.key,
            ));
            spans.push(Span::styled(
                ": ".to_string(),
                theme.fg_dim_style,
            ));
        }

        // Array index label
        if let Some(idx) = row.array_index {
            spans.push(Span::styled(
                format!("[{}] ", idx),
                theme.fg_dim_style,
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
                            theme.fg_dim_style,
                        ));
                        spans.push(Span::styled("}".to_string(), theme.bracket));
                    }
                }
                _ => {
                    spans.push(Span::styled("[".to_string(), theme.bracket));
                    if !row.is_expanded {
                        spans.push(Span::styled(
                            format!("{} items", child_count),
                            theme.fg_dim_style,
                        ));
                        spans.push(Span::styled("]".to_string(), theme.bracket));
                    }
                }
            }
        } else {
            match row.status {
                DiffStatus::Unchanged => {
                    if let Some(ref v) = row.right {
                        spans.push(Span::styled(value_display(v), theme.fg_style));
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
                        theme.fg_dim_style,
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
            Line::from(spans).style(theme.selection_style)
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
                theme.fg_dim_style,
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
            .style(theme.bg_style);
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

    fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.rows.len());
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::algo;
    use serde_json::json;

    /// Regression test: expanding node A, then expanding node B (whose children
    /// insert rows *above* A) must not collapse A.
    ///
    /// The old code tracked expanded state by row index. After B's expansion
    /// shifted rows, A's index changed but the expanded set still held the
    /// stale index, causing A to appear collapsed.
    #[test]
    fn expand_survives_row_index_shift() {
        // Two modified objects so both are expandable containers.
        let left = json!({"a": {"x": 1}, "b": {"y": 2}});
        let right = json!({"a": {"x": 1, "z": 3}, "b": {"y": 2, "w": 4}});
        let result = algo::diff(&left, &right);

        let mut view = DiffView::new(result);
        view.scroll.viewport = 50;

        // Step 1: expand root to reveal children "a" and "b".
        assert!(view.rows[0].is_container);
        view.expand(); // root is selected by default

        // Step 2: expand "b" (appears after "a" in sorted order).
        let b_idx = view.rows.iter().position(|r| r.key.as_deref() == Some("b")).unwrap();
        view.scroll.selected = b_idx;
        view.expand();
        assert!(view.rows[b_idx].is_expanded);

        // Step 3: expand "a" — its children insert rows BEFORE "b", shifting
        //         "b" to a higher row index.
        let a_idx = view.rows.iter().position(|r| r.key.as_deref() == Some("a")).unwrap();
        view.scroll.selected = a_idx;
        view.expand();

        // Verify: "b" must still be expanded despite its row index having changed.
        let b_idx_after = view.rows.iter().position(|r| r.key.as_deref() == Some("b")).unwrap();
        assert_ne!(b_idx, b_idx_after, "b should have moved to a different row index");
        assert!(view.rows[b_idx_after].is_expanded, "b must remain expanded after row shift");
    }

    #[test]
    fn collapse_after_row_shift() {
        let left = json!({"a": {"x": 1}, "b": {"y": 2}});
        let right = json!({"a": {"x": 1, "z": 3}, "b": {"y": 2, "w": 4}});
        let result = algo::diff(&left, &right);

        let mut view = DiffView::new(result);
        view.scroll.viewport = 50;

        // Expand root, then both children.
        view.expand();
        let a_idx = view.rows.iter().position(|r| r.key.as_deref() == Some("a")).unwrap();
        view.scroll.selected = a_idx;
        view.expand();
        let b_idx = view.rows.iter().position(|r| r.key.as_deref() == Some("b")).unwrap();
        view.scroll.selected = b_idx;
        view.expand();

        // Collapse "a" — rows shift, "b" moves up.
        let a_idx = view.rows.iter().position(|r| r.key.as_deref() == Some("a")).unwrap();
        view.scroll.selected = a_idx;
        view.collapse();

        // "b" must still be expanded.
        let b_idx_after = view.rows.iter().position(|r| r.key.as_deref() == Some("b")).unwrap();
        assert!(view.rows[b_idx_after].is_expanded, "b must remain expanded after a is collapsed");
    }
}
