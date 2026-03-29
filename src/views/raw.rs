use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
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

    pub fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.total_lines);
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
                    Style::new().fg(theme.fg_dim),
                )];

                // Simple syntax highlighting by character scanning
                spans.extend(highlight_json_line(content, theme));

                if is_selected {
                    Line::from(spans).style(
                        Style::new()
                            .bg(theme.selection_bg)
                            .fg(theme.selection_fg),
                    )
                } else {
                    Line::from(spans)
                }
            })
            .collect();

        let paragraph = ratatui::widgets::Paragraph::new(lines)
            .style(Style::new().bg(theme.bg));
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

    fn search_highlights(&self) -> &[NodeId] {
        &[]
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

/// Rebuild a serde_json::Value from our arena model (for pretty-printing).
pub(crate) fn rebuild_serde_value(doc: &JsonDocument, id: NodeId) -> serde_json::Value {
    use crate::model::node::JsonValue;
    let node = doc.node(id);
    match &node.value {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Number(n) => serde_json::Value::Number(n.clone()),
        JsonValue::String(s) => serde_json::Value::String(s.to_string()),
        JsonValue::Array(children) => {
            let items: Vec<serde_json::Value> = children
                .iter()
                .map(|&child_id| rebuild_serde_value(doc, child_id))
                .collect();
            serde_json::Value::Array(items)
        }
        JsonValue::Object(entries) => {
            let map: serde_json::Map<String, serde_json::Value> = entries
                .iter()
                .map(|(key, child_id)| (key.to_string(), rebuild_serde_value(doc, *child_id)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
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
                        Style::new().fg(theme.fg),
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
                        Style::new().fg(theme.fg),
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
                        Style::new().fg(theme.fg),
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
                    spans.push(Span::styled(word, Style::new().fg(theme.fg)));
                }
            }
            'n' => {
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        Style::new().fg(theme.fg),
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
                    spans.push(Span::styled(word, Style::new().fg(theme.fg)));
                }
            }
            c if c == '-' || c.is_ascii_digit() => {
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        Style::new().fg(theme.fg),
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
        spans.push(Span::styled(current, Style::new().fg(theme.fg)));
    }

    spans
}
