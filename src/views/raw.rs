use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::model::node::{JsonDocument, NodeId};
use crate::theme::Theme;
use crate::views::{StatusInfo, View, ViewAction};

/// Raw JSON view with syntax highlighting and line numbers.
pub struct RawView {
    lines: Vec<String>,
    scroll: crate::util::ScrollState,
    total_lines: usize,
}

impl RawView {
    pub fn new(document: &JsonDocument) -> Self {
        let json_value = rebuild_serde_value(document, document.root());
        let pretty = serde_json::to_string_pretty(&json_value).unwrap_or_default();
        let lines: Vec<String> = pretty.lines().map(|l| l.to_string()).collect();
        let total_lines = lines.len();

        Self {
            lines,
            scroll: crate::util::ScrollState::new(),
            total_lines,
        }
    }

}

impl View for RawView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = area.height as usize;
        let gutter_width = digit_count(self.total_lines) + 1;

        let start = self.scroll.offset;
        let end = (start + height).min(self.total_lines);

        let lines: Vec<Line> = (start..end)
            .map(|i| {
                let is_selected = i == self.scroll.selected;
                let line_num = format!("{:>width$} ", i + 1, width = gutter_width);
                let content = self.lines.get(i).map(|s| s.as_str()).unwrap_or("");

                let mut spans = vec![Span::styled(
                    line_num,
                    theme.fg_dim_style,
                )];

                // Simple syntax highlighting by character scanning
                spans.extend(highlight_json_line(content, theme));

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

        if self.total_lines > height {
            crate::ui::render_scrollbar(frame, area, self.total_lines, self.scroll.offset, theme);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                self.scroll.move_up();
            }
            (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                self.scroll.move_down(self.total_lines);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
                self.scroll.page_up(2);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
                self.scroll.page_down(self.total_lines, 2);
            }
            (KeyModifiers::NONE, KeyCode::Home) => self.scroll.go_top(),
            (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                self.scroll.go_bottom(self.total_lines);
            }
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        let pos = if self.total_lines == 0 { 0 } else { self.scroll.selected + 1 };
        StatusInfo {
            cursor_path: format!("line {}/{}", pos, self.total_lines),
            extra: None,
        }
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.total_lines);
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        let target = self.scroll.offset + row_in_viewport;
        if target < self.total_lines {
            self.scroll.selected = target;
            self.scroll.clamp(self.total_lines);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn digit_count(n: usize) -> usize {
    crate::util::digit_count(n)
}

/// Rebuild a `serde_json::Value` from our arena model (for pretty-printing).
///
/// Uses an explicit work-stack + value-stack to avoid stack overflow on deeply
/// nested JSON. Every other tree walk in this codebase is already iterative;
/// this brings the last holdout in line.
pub(crate) fn rebuild_serde_value(doc: &JsonDocument, id: NodeId) -> serde_json::Value {
    use crate::model::node::JsonValue;

    /// Deferred work items. `Visit` pushes leaf values or schedules children;
    /// `Build*` assembles children that have already been evaluated.
    enum Work {
        Visit(NodeId),
        BuildArray(usize),
        /// (node_id, entry_count) — keys are re-read from the arena at build
        /// time to avoid an intermediate `Vec<String>` allocation per object.
        BuildObject(NodeId, usize),
    }

    let mut work: Vec<Work> = vec![Work::Visit(id)];
    let mut values: Vec<serde_json::Value> = Vec::new();

    while let Some(item) = work.pop() {
        match item {
            Work::Visit(visit_id) => {
                let node = doc.node(visit_id);
                match &node.value {
                    JsonValue::Null => values.push(serde_json::Value::Null),
                    JsonValue::Bool(b) => values.push(serde_json::Value::Bool(*b)),
                    JsonValue::Number(n) => values.push(serde_json::Value::Number(n.clone())),
                    JsonValue::String(s) => values.push(serde_json::Value::String(s.to_string())),
                    JsonValue::Array(children) => {
                        work.push(Work::BuildArray(children.len()));
                        for &child_id in children.iter().rev() {
                            work.push(Work::Visit(child_id));
                        }
                    }
                    JsonValue::Object(entries) => {
                        work.push(Work::BuildObject(visit_id, entries.len()));
                        for (_, child_id) in entries.iter().rev() {
                            work.push(Work::Visit(*child_id));
                        }
                    }
                }
            }
            Work::BuildArray(count) => {
                let start = values.len() - count;
                let items = values.drain(start..).collect();
                values.push(serde_json::Value::Array(items));
            }
            Work::BuildObject(node_id, count) => {
                let entries = match &doc.node(node_id).value {
                    JsonValue::Object(e) => e,
                    _ => unreachable!("BuildObject issued for non-object node"),
                };
                let start = values.len() - count;
                let vals = values.drain(start..);
                let map = entries.iter().map(|(k, _)| k.to_string()).zip(vals).collect();
                values.push(serde_json::Value::Object(map));
            }
        }
    }

    values.pop().unwrap_or(serde_json::Value::Null)
}

/// Simple line-level JSON syntax highlighting.
fn highlight_json_line<'a>(line: &str, theme: &Theme) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current = String::new();

    while let Some(&ch) = chars.peek() {
        match ch {
            '"' => {
                // Flush pending
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        theme.fg_style,
                    ));
                }
                // Read quoted string
                let mut s = String::new();
                s.push(chars.next().unwrap()); // opening quote
                let mut escaped = false;
                while let Some(&c) = chars.peek() {
                    s.push(chars.next().unwrap());
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    }
                }
                // Determine if this is a key (followed by ':') or a value
                // Peek for colon, skipping whitespace
                let mut lookahead = chars.clone();
                let mut found_colon = false;
                while let Some(&c) = lookahead.peek() {
                    if c == ' ' {
                        lookahead.next();
                    } else {
                        found_colon = c == ':';
                        break;
                    }
                }
                if found_colon {
                    spans.push(Span::styled(s, theme.key));
                } else {
                    spans.push(Span::styled(s, theme.string));
                }
            }
            '{' | '}' | '[' | ']' => {
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        theme.fg_style,
                    ));
                }
                spans.push(Span::styled(
                    chars.next().unwrap().to_string(),
                    theme.bracket,
                ));
            }
            't' | 'f' => {
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        theme.fg_style,
                    ));
                }
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphabetic() {
                        word.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                if word == "true" || word == "false" {
                    spans.push(Span::styled(word, theme.boolean));
                } else {
                    spans.push(Span::styled(word, theme.fg_style));
                }
            }
            'n' => {
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        theme.fg_style,
                    ));
                }
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphabetic() {
                        word.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                if word == "null" {
                    spans.push(Span::styled(word, theme.null));
                } else {
                    spans.push(Span::styled(word, theme.fg_style));
                }
            }
            c if c == '-' || c.is_ascii_digit() => {
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        theme.fg_style,
                    ));
                }
                let mut num = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E'
                    {
                        num.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                spans.push(Span::styled(num, theme.number));
            }
            _ => {
                current.push(chars.next().unwrap());
            }
        }
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, theme.fg_style));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{DocumentMetadata, JsonNode, JsonValue, NodeId};
    use std::time::Duration;

    /// Build a document with `depth` levels of nested single-element arrays
    /// wrapping a null leaf, without recursion (so the test itself can't overflow).
    fn build_deep_array_chain(depth: usize) -> JsonDocument {
        let total = depth + 1; // depth array nodes + 1 null leaf
        let mut nodes = Vec::with_capacity(total);

        // Index 0: null at the deepest level.
        nodes.push(JsonNode {
            parent: None,
            value: JsonValue::Null,
            depth: depth as u16,
        });

        // Indices 1..=depth: array wrapping the previous node.
        for i in 1..total {
            let child_id = NodeId::from_raw((i - 1) as u32);
            let my_id = NodeId::from_raw(i as u32);
            nodes.push(JsonNode {
                parent: None,
                value: JsonValue::Array(vec![child_id]),
                depth: (depth - i) as u16,
            });
            nodes[child_id.index()].parent = Some(my_id);
        }

        let root = NodeId::from_raw((total - 1) as u32);
        JsonDocument::from_raw_parts(
            nodes,
            root,
            DocumentMetadata {
                source_path: None,
                source_size: 0,
                parse_time: Duration::ZERO,
                total_nodes: total,
                max_depth: depth as u16,
            },
        )
    }

    /// The iterative rebuild must handle nesting depths that would overflow
    /// the call stack in the old recursive version.
    #[test]
    fn rebuild_deeply_nested_does_not_overflow() {
        let depth = 5_000;
        let doc = build_deep_array_chain(depth);
        let value = rebuild_serde_value(&doc, doc.root());

        // Walk the result iteratively to verify structure.
        let mut v = &value;
        for _ in 0..depth {
            match v {
                serde_json::Value::Array(items) => {
                    assert_eq!(items.len(), 1);
                    v = &items[0];
                }
                other => panic!("expected array, got {:?}", other),
            }
        }
        assert!(v.is_null(), "innermost value should be null");
    }

    #[test]
    fn rebuild_simple_object() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"a": 1, "b": [true, null], "c": "hello"}"#,
        )
        .unwrap();
        let doc = crate::model::node::DocumentBuilder::from_serde_value(
            json.clone(),
            None,
            0,
            Duration::ZERO,
        );
        let rebuilt = rebuild_serde_value(&doc, doc.root());
        assert_eq!(json, rebuilt);
    }
}
