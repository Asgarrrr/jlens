use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;
use crate::views::{StatusInfo, View, ViewAction};

/// Flattened path view: shows all leaf values with their full JSON path.
/// e.g. `$.users[0].name = "Alice"`
pub struct PathView {
    #[allow(dead_code)]
    document: Arc<JsonDocument>,
    entries: Vec<PathEntry>,
    scroll: crate::util::ScrollState,
}

struct PathEntry {
    path: String,
    value: String,
    #[allow(dead_code)]
    node_id: NodeId,
}

impl PathView {
    pub fn new(document: Arc<JsonDocument>) -> Self {
        let mut entries = Vec::new();
        collect_leaves(&document, document.root(), "$".to_string(), &mut entries);

        Self {
            document,
            entries,
            scroll: crate::util::ScrollState::new(),
        }
    }

}

impl View for PathView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = area.height as usize;

        if self.entries.is_empty() {
            let paragraph = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                "No leaf values found",
                theme.fg_dim_style,
            )))
            .style(theme.bg_style);
            frame.render_widget(paragraph, area);
            return;
        }

        let start = self.scroll.offset;
        let end = (start + height).min(self.entries.len());

        let lines: Vec<Line> = (start..end)
            .map(|i| {
                let entry = &self.entries[i];
                let is_selected = i == self.scroll.selected;

                let spans = vec![
                    Span::styled(entry.path.clone(), theme.key),
                    Span::styled(
                        " = ".to_string(),
                        theme.fg_dim_style,
                    ),
                    Span::styled(
                        entry.value.clone(),
                        theme.style_for_leaf_value(&entry.value),
                    ),
                ];

                if is_selected {
                    Line::from(spans).style(theme.selection_style)
                } else {
                    Line::from(spans)
                }
            })
            .collect();

        let paragraph = ratatui::widgets::Paragraph::new(lines)
            .style(theme.bg_style);
        frame.render_widget(paragraph, area);

        if self.entries.len() > height {
            crate::ui::render_scrollbar(frame, area, self.entries.len(), self.scroll.offset, theme);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                self.scroll.move_up();
            }
            (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                self.scroll.move_down(self.entries.len());
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
                self.scroll.page_up(2);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
                self.scroll.page_down(self.entries.len(), 2);
            }
            (KeyModifiers::NONE, KeyCode::Home) => self.scroll.go_top(),
            (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                self.scroll.go_bottom(self.entries.len());
            }
            (KeyModifiers::NONE, KeyCode::Char('y')) => {
                if let Some(entry) = self.entries.get(self.scroll.selected) {
                    return ViewAction::CopyToClipboard(format!(
                        "{} = {}",
                        entry.path, entry.value
                    ));
                }
            }
            (KeyModifiers::SHIFT, KeyCode::Char('Y')) => {
                if let Some(entry) = self.entries.get(self.scroll.selected) {
                    return ViewAction::CopyToClipboard(entry.path.clone());
                }
            }
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        let total = self.entries.len();
        let path = self
            .entries
            .get(self.scroll.selected)
            .map(|e| e.path.clone())
            .unwrap_or_else(|| "$".to_string());
        let pos = if total == 0 { 0 } else { self.scroll.selected + 1 };
        StatusInfo {
            cursor_path: path,
            extra: Some(format!("{}/{} leaves", pos, total)),
        }
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.entries.len());
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        let target = self.scroll.offset + row_in_viewport;
        let total = self.entries.len();
        if target < total {
            self.scroll.selected = target;
            self.scroll.clamp(total);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Iteratively collect all leaf values with their full JSON paths.
fn collect_leaves(doc: &JsonDocument, root: NodeId, root_path: String, out: &mut Vec<PathEntry>) {
    let mut stack: Vec<(NodeId, String)> = vec![(root, root_path)];

    while let Some((id, path)) = stack.pop() {
        let node = doc.node(id);
        match &node.value {
            JsonValue::Array(children) => {
                // Push in reverse so we process left-to-right after popping.
                for (i, &child_id) in children.iter().enumerate().rev() {
                    stack.push((child_id, format!("{}[{}]", path, i)));
                }
            }
            JsonValue::Object(entries) => {
                for (key, child_id) in entries.iter().rev() {
                    stack.push((*child_id, format!("{}.{}", path, key)));
                }
            }
            _ => {
                let value = format_leaf_value(&node.value);
                out.push(PathEntry {
                    path,
                    value,
                    node_id: id,
                });
            }
        }
    }
}

fn format_leaf_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => {
            if s.chars().count() > 80 {
                format!("\"{}...\"", crate::util::truncate_chars(s, 77))
            } else {
                format!("\"{}\"", s)
            }
        }
        _ => unreachable!("collect_leaves only calls this for leaf nodes"),
    }
}

// Extension trait for theme to style leaf values by their string representation.
trait ThemeExt {
    fn style_for_leaf_value(&self, value_str: &str) -> Style;
}

impl ThemeExt for Theme {
    fn style_for_leaf_value(&self, value_str: &str) -> Style {
        if value_str == "null" {
            self.null
        } else if value_str == "true" || value_str == "false" {
            self.boolean
        } else if value_str.starts_with('"') {
            self.string
        } else {
            self.number
        }
    }
}
