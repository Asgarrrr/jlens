use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::keymap::Action;
use crate::model::node::{JsonDocument, NodeId};
use crate::theme::Theme;
use crate::views::{StatusInfo, View, ViewAction};

/// Raw JSON view with syntax highlighting and line numbers.
///
/// Stores a single pretty-printed String with byte offsets per line
/// rather than `Vec<String>`, eliminating N heap allocations.
pub struct RawView {
    pretty: String,
    line_offsets: Vec<u32>,
    scroll: crate::util::ScrollState,
    scroll_x: usize,
}

impl RawView {
    pub fn new(document: &JsonDocument, root: NodeId) -> Self {
        let json_value = rebuild_serde_value(document, root);
        let pretty = serde_json::to_string_pretty(&json_value).unwrap_or_default();

        let mut offsets = vec![0u32];
        for (i, &byte) in pretty.as_bytes().iter().enumerate() {
            if byte == b'\n' {
                offsets.push((i + 1) as u32);
            }
        }

        Self {
            pretty,
            line_offsets: offsets,
            scroll_x: 0,
            scroll: crate::util::ScrollState::new(),
        }
    }

    fn total_lines(&self) -> usize {
        self.line_offsets.len()
    }

    fn line(&self, idx: usize) -> &str {
        let start = self.line_offsets[idx] as usize;
        let end = self
            .line_offsets
            .get(idx + 1)
            .map(|&o| o as usize)
            .unwrap_or(self.pretty.len());
        self.pretty[start..end].trim_end_matches('\n')
    }
}

impl View for RawView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = area.height as usize;
        let gutter_width = crate::util::digit_count(self.total_lines()) + 1;

        let start = self.scroll.offset;
        let end = (start + height).min(self.total_lines());

        let lines: Vec<Line> = (start..end)
            .map(|i| {
                let is_selected = i == self.scroll.selected;
                let line_num = format!("{:>width$} ", i + 1, width = gutter_width);
                let content = if i < self.total_lines() {
                    self.line(i)
                } else {
                    ""
                };

                let mut spans = vec![
                    Span::styled(line_num, theme.fg_dim_style),
                    Span::styled("│ ", theme.tree_guide_style),
                ];

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
            .style(theme.bg_style)
            .scroll((0, self.scroll_x as u16));
        frame.render_widget(paragraph, area);

        if self.total_lines() > height {
            crate::ui::render_scrollbar(frame, area, self.total_lines(), self.scroll.offset, theme);
        }
    }

    fn handle_action(&mut self, action: Action) -> ViewAction {
        match action {
            Action::MoveUp => self.scroll.move_up(),
            Action::MoveDown => self.scroll.move_down(self.total_lines()),
            Action::PageUp => self.scroll.page_up(2),
            Action::PageDown => self.scroll.page_down(self.total_lines(), 2),
            Action::Home => self.scroll.go_top(),
            Action::End => self.scroll.go_bottom(self.total_lines()),
            Action::ScrollLeft => self.scroll_x = self.scroll_x.saturating_sub(4),
            Action::ScrollRight => { self.scroll_x += 4; }
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        let pos = if self.total_lines() == 0 {
            0
        } else {
            self.scroll.selected + 1
        };
        StatusInfo {
            cursor_path: format!("line {}/{}", pos, self.total_lines()),
        }
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.total_lines());
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        let target = self.scroll.offset + row_in_viewport;
        if target < self.total_lines() {
            self.scroll.selected = target;
            self.scroll.clamp(self.total_lines());
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
                let map = entries
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .zip(vals)
                    .collect();
                values.push(serde_json::Value::Object(map));
            }
        }
    }

    values.pop().unwrap_or(serde_json::Value::Null)
}

/// Simple line-level JSON syntax highlighting.
///
/// Borrows slices of `line` directly — no String allocations per token.
fn highlight_json_line<'a>(line: &'a str, theme: &Theme) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    // Helper: advance `pos` past a run of bytes matching `pred`, return the
    // borrowed slice.  All characters we care about are ASCII, so byte-level
    // scanning is correct for the full UTF-8 string.

    while pos < len {
        let b = bytes[pos];

        match b {
            // Leading / mid-token whitespace — emit as a single borrow.
            b' ' | b'\t' => {
                let start = pos;
                while pos < len && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                    pos += 1;
                }
                spans.push(Span::styled(&line[start..pos], theme.fg_style));
            }

            // Quoted string (key or value).
            b'"' => {
                let start = pos;
                pos += 1; // skip opening quote
                let mut escaped = false;
                while pos < len {
                    let c = bytes[pos];
                    pos += 1;
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        break; // closing quote consumed
                    }
                }
                // Look ahead past optional spaces for ':'
                let mut look = pos;
                while look < len && bytes[look] == b' ' {
                    look += 1;
                }
                let is_key = look < len && bytes[look] == b':';
                let style = if is_key { theme.key } else { theme.string };
                spans.push(Span::styled(&line[start..pos], style));
            }

            // Brackets / braces — single ASCII byte, safe to slice by byte offset.
            b'{' | b'}' | b'[' | b']' => {
                spans.push(Span::styled(&line[pos..pos + 1], theme.bracket));
                pos += 1;
            }

            // Comma, colon — punctuation with default fg style.
            b',' | b':' => {
                spans.push(Span::styled(&line[pos..pos + 1], theme.fg_style));
                pos += 1;
            }

            // Keywords: true / false / null, and any other alphabetic run.
            b't' | b'f' | b'n' | b'a'..=b'z' | b'A'..=b'Z' => {
                let start = pos;
                while pos < len && bytes[pos].is_ascii_alphabetic() {
                    pos += 1;
                }
                let word = &line[start..pos];
                let style = match word {
                    "true" | "false" => theme.boolean,
                    "null" => theme.null,
                    _ => theme.fg_style,
                };
                spans.push(Span::styled(word, style));
            }

            // Numbers: optional leading minus, digits, '.', 'e'/'E', '+'/'-'.
            b'-' | b'0'..=b'9' => {
                let start = pos;
                while pos < len {
                    match bytes[pos] {
                        b'0'..=b'9' | b'.' | b'-' | b'+' | b'e' | b'E' => pos += 1,
                        _ => break,
                    }
                }
                spans.push(Span::styled(&line[start..pos], theme.number));
            }

            // Anything else (should not normally appear in pretty-printed JSON,
            // but handle gracefully by borrowing one UTF-8 character at a time).
            _ => {
                let start = pos;
                // Advance by one Unicode scalar value to keep slices valid.
                let ch = line[pos..].chars().next().unwrap_or('\0');
                pos += ch.len_utf8();
                spans.push(Span::styled(&line[start..pos], theme.fg_style));
            }
        }
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
        let json: serde_json::Value =
            serde_json::from_str(r#"{"a": 1, "b": [true, null], "c": "hello"}"#).unwrap();
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
