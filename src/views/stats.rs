use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;
use crate::views::{StatusInfo, View, ViewAction};

/// Stats/summary view showing document-level metrics.
pub struct StatsView {
    lines: Vec<StatsLine>,
    scroll: crate::util::ScrollState,
}

struct DocumentStats {
    total_nodes: usize,
    max_depth: u16,
    avg_depth: f64,
    type_counts: HashMap<&'static str, usize>,
    total_string_bytes: usize,
    longest_string: usize,
    largest_array: (String, usize),
    largest_object: (String, usize),
    unique_keys: usize,
    total_keys: usize,
}

enum StatsLine {
    Header(String),
    KeyValue(String, String),
    Bar(String, Vec<(String, f64, Style)>),
    Blank,
}

impl StatsView {
    pub fn new(document: Arc<JsonDocument>, theme: &Theme) -> Self {
        let stats = compute_stats(&document);
        let lines = build_stats_lines(&stats, document.metadata(), theme);

        Self {
            lines,
            scroll: crate::util::ScrollState::new(),
        }
    }

    pub fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.lines.len());
    }
}

impl View for StatsView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = area.height as usize;
        let width = area.width as usize;

        let start = self.scroll.offset;
        let end = (start + height).min(self.lines.len());

        let rendered_lines: Vec<Line> = (start..end)
            .map(|i| {
                let is_selected = i == self.scroll.selected;
                let line = match &self.lines[i] {
                    StatsLine::Header(text) => Line::from(Span::styled(
                        format!("  {} ", text),
                        Style::new()
                            .fg(theme.toolbar_active_bg)
                            .add_modifier(Modifier::BOLD),
                    )),
                    StatsLine::KeyValue(key, value) => Line::from(vec![
                        Span::styled(
                            format!("    {:<24}", key),
                            Style::new().fg(theme.fg_dim),
                        ),
                        Span::styled(value.clone(), Style::new().fg(theme.fg)),
                    ]),
                    StatsLine::Bar(label, segments) => {
                        let mut spans = vec![Span::styled(
                            format!("    {:<24}", label),
                            Style::new().fg(theme.fg_dim),
                        )];
                        let bar_width = width.saturating_sub(28);
                        for (name, ratio, style) in segments {
                            let segment_width = ((bar_width as f64) * ratio).round() as usize;
                            if segment_width > 0 {
                                let bar_text = if segment_width >= name.len() + 2 {
                                    format!("{:^width$}", name, width = segment_width)
                                } else {
                                    "█".repeat(segment_width)
                                };
                                spans.push(Span::styled(bar_text, *style));
                            }
                        }
                        Line::from(spans)
                    }
                    StatsLine::Blank => Line::from(""),
                };

                if is_selected {
                    line.style(
                        Style::new()
                            .bg(theme.selection_bg)
                            .fg(theme.selection_fg),
                    )
                } else {
                    line
                }
            })
            .collect();

        let paragraph = ratatui::widgets::Paragraph::new(rendered_lines)
            .style(Style::new().bg(theme.bg));
        frame.render_widget(paragraph, area);

        if self.lines.len() > height {
            crate::ui::render_scrollbar(frame, area, self.lines.len(), self.scroll.offset, theme);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                self.scroll.move_up();
            }
            (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                self.scroll.move_down(self.lines.len());
            }
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        StatusInfo {
            cursor_path: "Document Statistics".to_string(),
            extra: None,
        }
    }

    fn search_highlights(&self) -> &[NodeId] {
        &[]
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        let target = self.scroll.offset + row_in_viewport;
        let total = self.lines.len();
        if target < total {
            self.scroll.selected = target;
            self.scroll.clamp(total);
        }
    }
}

// ---------------------------------------------------------------------------
// Stats computation
// ---------------------------------------------------------------------------

fn compute_stats(doc: &JsonDocument) -> DocumentStats {
    let mut type_counts: HashMap<&'static str, usize> = HashMap::new();
    let mut total_string_bytes: usize = 0;
    let mut longest_string: usize = 0;
    let mut largest_array: (String, usize) = (String::new(), 0);
    let mut largest_object: (String, usize) = (String::new(), 0);
    let mut key_counts: HashMap<Arc<str>, usize> = HashMap::new();
    let mut depth_sum: u64 = 0;
    let mut node_count: u64 = 0;

    visit_all(doc, doc.root(), &mut |doc, id| {
        let node = doc.node(id);
        node_count += 1;
        depth_sum += node.depth as u64;

        let type_name = node.value.type_name();
        *type_counts.entry(type_name).or_default() += 1;

        match &node.value {
            JsonValue::String(s) => {
                total_string_bytes += s.len();
                longest_string = longest_string.max(s.len());
            }
            JsonValue::Array(children) => {
                if children.len() > largest_array.1 {
                    largest_array = (doc.path_of(id), children.len());
                }
            }
            JsonValue::Object(entries) => {
                if entries.len() > largest_object.1 {
                    largest_object = (doc.path_of(id), entries.len());
                }
                for (key, _) in entries {
                    *key_counts.entry(key.clone()).or_default() += 1;
                }
            }
            _ => {}
        }
    });

    let avg_depth = if node_count > 0 {
        depth_sum as f64 / node_count as f64
    } else {
        0.0
    };

    DocumentStats {
        total_nodes: doc.metadata().total_nodes,
        max_depth: doc.metadata().max_depth,
        avg_depth,
        type_counts,
        total_string_bytes,
        longest_string,
        largest_array,
        largest_object,
        unique_keys: key_counts.len(),
        total_keys: key_counts.values().sum(),
    }
}

fn visit_all(doc: &JsonDocument, root: NodeId, visitor: &mut impl FnMut(&JsonDocument, NodeId)) {
    let mut stack: Vec<NodeId> = vec![root];
    while let Some(id) = stack.pop() {
        visitor(doc, id);
        let node = doc.node(id);
        match &node.value {
            JsonValue::Array(children) => {
                for &child_id in children.iter().rev() {
                    stack.push(child_id);
                }
            }
            JsonValue::Object(entries) => {
                for &child_id in entries.iter().map(|(_, id)| id).rev() {
                    stack.push(child_id);
                }
            }
            _ => {}
        }
    }
}

fn build_stats_lines(
    stats: &DocumentStats,
    metadata: &crate::model::node::DocumentMetadata,
    theme: &Theme,
) -> Vec<StatsLine> {
    let mut lines = Vec::new();

    // Document overview
    lines.push(StatsLine::Header("Document Overview".to_string()));
    lines.push(StatsLine::Blank);

    if let Some(ref path) = metadata.source_path {
        lines.push(StatsLine::KeyValue(
            "File".to_string(),
            path.display().to_string(),
        ));
    }
    lines.push(StatsLine::KeyValue(
        "File size".to_string(),
        humansize::format_size(metadata.source_size, humansize::BINARY),
    ));
    lines.push(StatsLine::KeyValue(
        "Parse time".to_string(),
        format!("{}ms", metadata.parse_time.as_millis()),
    ));
    lines.push(StatsLine::KeyValue(
        "Total nodes".to_string(),
        format_count(stats.total_nodes),
    ));
    lines.push(StatsLine::KeyValue(
        "Max depth".to_string(),
        stats.max_depth.to_string(),
    ));
    lines.push(StatsLine::KeyValue(
        "Avg depth".to_string(),
        format!("{:.1}", stats.avg_depth),
    ));

    lines.push(StatsLine::Blank);
    lines.push(StatsLine::Header("Type Distribution".to_string()));
    lines.push(StatsLine::Blank);

    let total = stats.total_nodes as f64;
    let type_order = ["object", "array", "string", "number", "bool", "null"];
    for type_name in type_order {
        let count = stats.type_counts.get(type_name).copied().unwrap_or(0);
        if count > 0 {
            let pct = (count as f64 / total) * 100.0;
            lines.push(StatsLine::KeyValue(
                type_name.to_string(),
                format!("{} ({:.1}%)", format_count(count), pct),
            ));
        }
    }

    // Type distribution bar
    if total > 0.0 {
        lines.push(StatsLine::Blank);
        let segments: Vec<(String, f64, Style)> = type_order
            .iter()
            .filter_map(|&t| {
                let count = stats.type_counts.get(t).copied().unwrap_or(0);
                if count == 0 {
                    return None;
                }
                let ratio = count as f64 / total;
                let bar_bg = match t {
                    "object" => theme.key.fg,
                    "array" => theme.boolean.fg,
                    "string" => theme.string.fg,
                    "number" => theme.number.fg,
                    "bool" => theme.bracket.fg,
                    "null" => theme.null.fg,
                    _ => None,
                };
                let style = match bar_bg {
                    Some(bg) => Style::new().bg(bg).fg(theme.bg),
                    None => Style::new(),
                };
                Some((t.to_string(), ratio, style))
            })
            .collect();
        lines.push(StatsLine::Bar("distribution".to_string(), segments));
    }

    // Strings
    lines.push(StatsLine::Blank);
    lines.push(StatsLine::Header("Strings".to_string()));
    lines.push(StatsLine::Blank);
    lines.push(StatsLine::KeyValue(
        "Total string bytes".to_string(),
        humansize::format_size(stats.total_string_bytes as u64, humansize::BINARY),
    ));
    lines.push(StatsLine::KeyValue(
        "Longest string".to_string(),
        format!("{} chars", format_count(stats.longest_string)),
    ));

    // Keys
    lines.push(StatsLine::Blank);
    lines.push(StatsLine::Header("Keys".to_string()));
    lines.push(StatsLine::Blank);
    lines.push(StatsLine::KeyValue(
        "Unique keys".to_string(),
        format_count(stats.unique_keys),
    ));
    lines.push(StatsLine::KeyValue(
        "Total key usages".to_string(),
        format_count(stats.total_keys),
    ));

    // Notable
    lines.push(StatsLine::Blank);
    lines.push(StatsLine::Header("Notable Structures".to_string()));
    lines.push(StatsLine::Blank);
    if stats.largest_array.1 > 0 {
        lines.push(StatsLine::KeyValue(
            "Largest array".to_string(),
            format!(
                "{} ({} items)",
                stats.largest_array.0,
                format_count(stats.largest_array.1)
            ),
        ));
    }
    if stats.largest_object.1 > 0 {
        lines.push(StatsLine::KeyValue(
            "Largest object".to_string(),
            format!(
                "{} ({} keys)",
                stats.largest_object.0,
                format_count(stats.largest_object.1)
            ),
        ));
    }

    lines
}

fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}
