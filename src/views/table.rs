use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, Table};

use crate::keymap::Action;
use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;
use crate::views::{StatusInfo, View, ViewAction};

/// Table view for arrays of objects.
/// Auto-discovers columns from the first N objects and renders them as a sortable table.
pub struct TableView {
    columns: Vec<Arc<str>>,
    rows: Vec<Vec<Option<String>>>,
    scroll: crate::util::ScrollState,
    fallback_message: Option<String>,
    sort_column: Option<usize>,
    sort_ascending: bool,
    /// Maps display row index → original row index.
    sorted_indices: Vec<usize>,
}

/// Result of searching for table-compatible data in the document.
enum TableData {
    Found {
        columns: Vec<Arc<str>>,
        rows: Vec<Vec<Option<String>>>,
    },
    NotFound(String),
}

/// Search the document for an array of objects suitable for table display.
fn find_table_data(doc: &JsonDocument) -> TableData {
    let root = doc.root();
    let root_node = doc.node(root);

    // Case 1: root is an array of objects
    if let JsonValue::Array(children) = &root_node.value
        && is_array_of_objects(doc, children)
    {
        let (columns, rows) = build_table(doc, children);
        return TableData::Found { columns, rows };
    }

    // Case 2: root is an object — find first child that is an array of objects
    if let JsonValue::Object(entries) = &root_node.value {
        for (_, child_id) in entries {
            let child = doc.node(*child_id);
            if let JsonValue::Array(children) = &child.value
                && is_array_of_objects(doc, children)
            {
                let (columns, rows) = build_table(doc, children);
                return TableData::Found { columns, rows };
            }
        }
    }

    TableData::NotFound(
        "No array of objects found. Table view requires an array of objects.".to_string(),
    )
}

impl TableView {
    pub fn new(document: Arc<JsonDocument>) -> Self {
        let data = find_table_data(&document);

        match data {
            TableData::Found { columns, rows } => {
                let sorted_indices = (0..rows.len()).collect();
                Self {
                    columns,
                    rows,
                    scroll: crate::util::ScrollState::new(),
                    fallback_message: None,
                    sort_column: None,
                    sort_ascending: true,
                    sorted_indices,
                }
            }
            TableData::NotFound(msg) => Self {
                columns: Vec::new(),
                rows: Vec::new(),
                scroll: crate::util::ScrollState::new(),
                fallback_message: Some(msg),
                sort_column: None,
                sort_ascending: true,
                sorted_indices: Vec::new(),
            },
        }
    }

    // -----------------------------------------------------------------------
    // Sorting
    // -----------------------------------------------------------------------

    fn cycle_sort_column_forward(&mut self) {
        if self.columns.is_empty() {
            return;
        }
        self.sort_column = Some(match self.sort_column {
            None => 0,
            Some(i) => (i + 1) % self.columns.len(),
        });
        self.apply_sort();
    }

    fn cycle_sort_column_backward(&mut self) {
        if self.columns.is_empty() {
            return;
        }
        self.sort_column = Some(match self.sort_column {
            None => self.columns.len() - 1,
            Some(0) => self.columns.len() - 1,
            Some(i) => i - 1,
        });
        self.apply_sort();
    }

    fn toggle_sort_direction(&mut self) {
        self.sort_ascending = !self.sort_ascending;
        self.apply_sort();
    }

    fn apply_sort(&mut self) {
        self.sorted_indices = (0..self.rows.len()).collect();
        if let Some(col) = self.sort_column {
            let ascending = self.sort_ascending;
            let rows = &self.rows;
            self.sorted_indices.sort_by(|&a, &b| {
                let va = rows[a].get(col).and_then(|v| v.as_deref()).unwrap_or("");
                let vb = rows[b].get(col).and_then(|v| v.as_deref()).unwrap_or("");
                // Try numeric comparison first, fall back to lexicographic.
                let cmp = match (va.parse::<f64>(), vb.parse::<f64>()) {
                    (Ok(na), Ok(nb)) => na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal),
                    _ => va.cmp(vb),
                };
                if ascending { cmp } else { cmp.reverse() }
            });
        }
        self.scroll.go_top();
    }
}

impl View for TableView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if let Some(ref msg) = self.fallback_message {
            let paragraph = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                msg.clone(),
                theme.fg_dim_style,
            )))
            .style(theme.bg_style);
            frame.render_widget(paragraph, area);
            return;
        }

        if self.columns.is_empty() {
            return;
        }

        let available_width = area.width as usize;
        let col_count = self.columns.len();
        // Reserve 3 chars per separator between columns (" │ ") when there are multiple columns.
        let separator_total = if col_count > 1 {
            (col_count - 1) * 3
        } else {
            0
        };
        let col_width = ((available_width.saturating_sub(separator_total)) / col_count).max(8);

        // Build header as a single line with separators between column labels.
        let header_spans: Vec<Span> = self
            .columns
            .iter()
            .enumerate()
            .flat_map(|(col_idx, c)| {
                let label = if self.sort_column == Some(col_idx) {
                    let indicator = if self.sort_ascending {
                        " \u{25b2}"
                    } else {
                        " \u{25bc}"
                    };
                    format!("{}{}", c, indicator)
                } else {
                    c.to_string()
                };
                // Pad/truncate label to col_width.
                let display = pad_or_truncate(&label, col_width);
                let mut parts: Vec<Span> = Vec::new();
                if col_idx > 0 {
                    parts.push(Span::styled(" \u{2502} ", theme.tree_guide_style));
                }
                parts.push(Span::styled(display, theme.toolbar_brand_style));
                parts
            })
            .collect();
        let header = Row::new(vec![Cell::from(Line::from(header_spans))])
            .style(theme.toolbar_active_style)
            .height(1);

        let visible_rows = self.scroll.viewport;
        let start = self.scroll.offset;
        let end = (start + visible_rows).min(self.sorted_indices.len());

        let data_rows: Vec<Row> = (start..end)
            .map(|i| {
                let is_selected = i == self.scroll.selected;
                let orig = self.sorted_indices[i];

                let row_style: Style = if is_selected {
                    theme.selection_style
                } else if (i % 2) == 1 {
                    theme.alt_row_bg
                } else {
                    theme.bg_style
                };

                let spans: Vec<Span> = self.rows[orig]
                    .iter()
                    .enumerate()
                    .flat_map(|(col_idx, cell_val)| {
                        let text = cell_val.as_deref().unwrap_or("\u{2014}");
                        let max_chars = col_width.saturating_sub(2);
                        let truncated = if text.chars().count() > max_chars {
                            let cut =
                                crate::util::truncate_chars(text, max_chars.saturating_sub(1));
                            format!("{}\u{2026}", cut)
                        } else {
                            text.to_string()
                        };
                        let display = pad_or_truncate(&truncated, col_width);
                        let mut parts: Vec<Span> = Vec::new();
                        if col_idx > 0 {
                            parts.push(Span::styled(" \u{2502} ", theme.tree_guide_style));
                        }
                        parts.push(Span::styled(display, theme.fg_style));
                        parts
                    })
                    .collect();

                Row::new(vec![Cell::from(Line::from(spans))]).style(row_style)
            })
            .collect();

        let widths = vec![Constraint::Min(available_width as u16)];

        let table = Table::new(data_rows, widths)
            .header(header)
            .style(theme.bg_style);

        frame.render_widget(table, area);

        // Scrollbar for the data area (below the header row)
        let data_height = area.height.saturating_sub(1) as usize;
        if data_height > 0 && self.rows.len() > data_height {
            let scrollbar_area = Rect::new(
                area.x,
                area.y + 1,
                area.width,
                area.height.saturating_sub(1),
            );
            crate::ui::render_scrollbar(
                frame,
                scrollbar_area,
                self.rows.len(),
                self.scroll.offset,
                theme,
            );
        }
    }

    fn handle_action(&mut self, action: Action) -> ViewAction {
        match action {
            Action::MoveUp => self.scroll.move_up(),
            Action::MoveDown => self.scroll.move_down(self.rows.len()),
            Action::PageUp => self.scroll.page_up(3),
            Action::PageDown => self.scroll.page_down(self.rows.len(), 3),
            Action::Home => self.scroll.go_top(),
            Action::End => self.scroll.go_bottom(self.rows.len()),
            Action::NextColumn => self.cycle_sort_column_forward(),
            Action::PrevColumn => self.cycle_sort_column_backward(),
            Action::CycleSort => self.toggle_sort_direction(),
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        let total = self.rows.len();
        let extra = if let Some(col) = self.sort_column {
            let col_name = self.columns.get(col).map(|c| c.as_ref()).unwrap_or("?");
            let dir = if self.sort_ascending { "asc" } else { "desc" };
            format!(
                "{} columns | sorted by {} ({})",
                self.columns.len(),
                col_name,
                dir
            )
        } else {
            format!("{} columns", self.columns.len())
        };
        StatusInfo {
            cursor_path: format!("row {}/{}", self.scroll.selected + 1, total),
            extra: Some(extra),
        }
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height.saturating_sub(3);
        self.scroll.clamp(self.rows.len());
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        let target = self.scroll.offset + row_in_viewport;
        let total = self.rows.len();
        if target < total {
            self.scroll.selected = target;
            self.scroll.clamp(total);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_array_of_objects(doc: &JsonDocument, children: &[NodeId]) -> bool {
    if children.is_empty() {
        return false;
    }
    let object_count = children
        .iter()
        .filter(|&&id| matches!(doc.node(id).value, JsonValue::Object(_)))
        .count();
    object_count > children.len() / 2
}

fn build_table(
    doc: &JsonDocument,
    children: &[NodeId],
) -> (Vec<Arc<str>>, Vec<Vec<Option<String>>>) {
    let mut column_set: Vec<Arc<str>> = Vec::new();
    let scan_limit = children.len().min(100);

    for &child_id in &children[..scan_limit] {
        let node = doc.node(child_id);
        if let JsonValue::Object(entries) = &node.value {
            for (key, _) in entries {
                if !column_set.iter().any(|k| k == key) {
                    column_set.push(key.clone());
                }
            }
        }
    }

    let rows: Vec<Vec<Option<String>>> = children
        .iter()
        .map(|&child_id| {
            let node = doc.node(child_id);
            column_set
                .iter()
                .map(|col| {
                    if let JsonValue::Object(entries) = &node.value {
                        entries
                            .iter()
                            .find(|(k, _)| k == col)
                            .map(|(_, val_id)| value_preview(doc, *val_id))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect();

    (column_set, rows)
}

/// Pad a string with spaces to `width` terminal columns, or truncate with
/// ellipsis if longer. Uses `display_width` so that wide (CJK) characters
/// are accounted for correctly.
fn pad_or_truncate(s: &str, width: usize) -> String {
    let dw = crate::util::display_width(s);
    if dw >= width {
        if width == 0 {
            return String::new();
        }
        // Truncate by display width: walk chars, accumulating column widths.
        let target = width.saturating_sub(1); // reserve 1 column for ellipsis
        let mut cols = 0usize;
        let mut byte_end = 0usize;
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if cols + cw > target {
                break;
            }
            cols += cw;
            byte_end += ch.len_utf8();
        }
        format!("{}\u{2026}", &s[..byte_end])
    } else {
        // Pad: dw < width, so we need (width - dw) spaces.
        format!("{}{}", s, " ".repeat(width - dw))
    }
}

fn value_preview(doc: &JsonDocument, id: NodeId) -> String {
    let node = doc.node(id);
    match &node.value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => {
            if s.chars().count() > 40 {
                format!("{}\u{2026}", crate::util::truncate_chars(s, 39))
            } else {
                s.to_string()
            }
        }
        JsonValue::Array(c) => format!("[{} items]", c.len()),
        JsonValue::Object(e) => format!("{{{} keys}}", e.len()),
    }
}
