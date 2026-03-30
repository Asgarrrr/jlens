use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Sparkline;
use ratatui::Frame;

use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;

/// What the preview pane should display for a given node.
pub(crate) enum PreviewContent {
    Sparkline {
        values: Vec<u64>,
        min: f64,
        max: f64,
        avg: f64,
        count: usize,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        total: usize,
    },
    StringList {
        items: Vec<String>,
        total: usize,
    },
    KeySummary {
        entries: Vec<(String, &'static str, String)>,
    },
    FormattedString {
        text: String,
        kind: StringKind,
    },
    Scalar {
        value: String,
        type_name: &'static str,
    },
}

pub(crate) enum StringKind {
    Plain,
    Url,
    IsoDate,
    Json,
}

/// Analyze a node from the arena and decide how to preview it.
pub(crate) fn analyze(doc: &JsonDocument, node: NodeId) -> PreviewContent {
    let n = doc.node(node);
    match &n.value {
        JsonValue::Array(children) => analyze_array(doc, children),
        JsonValue::Object(entries) => analyze_object(doc, entries),
        JsonValue::String(s) => analyze_string(s),
        JsonValue::Number(num) => PreviewContent::Scalar {
            value: num.to_string(),
            type_name: "number",
        },
        JsonValue::Bool(b) => PreviewContent::Scalar {
            value: b.to_string(),
            type_name: "boolean",
        },
        JsonValue::Null => PreviewContent::Scalar {
            value: "null".into(),
            type_name: "null",
        },
    }
}

fn analyze_array(doc: &JsonDocument, children: &[NodeId]) -> PreviewContent {
    if children.is_empty() {
        return PreviewContent::Scalar {
            value: "[] (empty)".into(),
            type_name: "array",
        };
    }

    // Sample first 100 to determine dominant type
    let sample = children.len().min(100);
    let mut num_count = 0;
    let mut obj_count = 0;
    let mut str_count = 0;

    for &id in &children[..sample] {
        match &doc.node(id).value {
            JsonValue::Number(_) => num_count += 1,
            JsonValue::Object(_) => obj_count += 1,
            JsonValue::String(_) => str_count += 1,
            _ => {}
        }
    }

    let threshold = sample * 80 / 100; // 80%

    if num_count >= threshold {
        build_sparkline(doc, children)
    } else if obj_count >= threshold {
        build_table(doc, children)
    } else if str_count >= threshold {
        build_string_list(doc, children)
    } else {
        PreviewContent::Scalar {
            value: format!("[{} items]", children.len()),
            type_name: "array",
        }
    }
}

fn build_sparkline(doc: &JsonDocument, children: &[NodeId]) -> PreviewContent {
    let mut values = Vec::with_capacity(children.len().min(1000));
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;

    for &id in children.iter().take(1000) {
        if let JsonValue::Number(n) = &doc.node(id).value {
            let v = n.as_f64().unwrap_or(0.0);
            min = min.min(v);
            max = max.max(v);
            sum += v;
            values.push(v);
        }
    }

    let count = values.len();
    let avg = if count > 0 { sum / count as f64 } else { 0.0 };
    let range = max - min;

    // Normalize to 0..100
    let normalized: Vec<u64> = values
        .iter()
        .map(|&v| {
            if range > 0.0 {
                (((v - min) / range) * 100.0) as u64
            } else {
                50
            }
        })
        .collect();

    PreviewContent::Sparkline {
        values: normalized,
        min,
        max,
        avg,
        count: children.len(),
    }
}

fn build_table(doc: &JsonDocument, children: &[NodeId]) -> PreviewContent {
    // Collect headers from first object
    let mut headers = Vec::new();
    if let JsonValue::Object(entries) = &doc.node(children[0]).value {
        for (key, _) in entries {
            headers.push(key.to_string());
        }
    }

    // Build rows (first 10)
    let mut rows = Vec::new();
    for &id in children.iter().take(10) {
        if let JsonValue::Object(entries) = &doc.node(id).value {
            let row: Vec<String> = headers
                .iter()
                .map(|h| {
                    entries
                        .iter()
                        .find(|(k, _)| k.as_ref() == h.as_str())
                        .map(|(_, vid)| value_preview(doc, *vid))
                        .unwrap_or_else(|| "\u{2014}".into())
                })
                .collect();
            rows.push(row);
        }
    }

    PreviewContent::Table {
        headers,
        rows,
        total: children.len(),
    }
}

fn build_string_list(doc: &JsonDocument, children: &[NodeId]) -> PreviewContent {
    let items: Vec<String> = children
        .iter()
        .take(20)
        .filter_map(|&id| {
            if let JsonValue::String(s) = &doc.node(id).value {
                Some(s.to_string())
            } else {
                None
            }
        })
        .collect();

    PreviewContent::StringList {
        total: children.len(),
        items,
    }
}

fn analyze_object(
    doc: &JsonDocument,
    entries: &[(std::sync::Arc<str>, NodeId)],
) -> PreviewContent {
    let mut summary = Vec::new();
    for (key, id) in entries.iter().take(20) {
        let node = doc.node(*id);
        let type_name = node.value.type_name();
        let preview = value_preview(doc, *id);
        summary.push((key.to_string(), type_name, preview));
    }
    PreviewContent::KeySummary { entries: summary }
}

fn analyze_string(s: &str) -> PreviewContent {
    let kind = if s.starts_with("http://") || s.starts_with("https://") {
        StringKind::Url
    } else if looks_like_iso_date(s) {
        StringKind::IsoDate
    } else if (s.starts_with('{') || s.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(s).is_ok()
    {
        StringKind::Json
    } else {
        StringKind::Plain
    };

    PreviewContent::FormattedString {
        text: s.to_string(),
        kind,
    }
}

fn looks_like_iso_date(s: &str) -> bool {
    // Quick check: 2024-01-15T... or 2024-01-15
    s.len() >= 10
        && s.as_bytes().get(4) == Some(&b'-')
        && s.as_bytes().get(7) == Some(&b'-')
        && s[..4].bytes().all(|b| b.is_ascii_digit())
}

fn value_preview(doc: &JsonDocument, id: NodeId) -> String {
    match &doc.node(id).value {
        JsonValue::Null => "null".into(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => {
            if s.len() > 40 {
                format!("\"{}...\"", crate::util::truncate_chars(s, 37))
            } else {
                format!("\"{}\"", s)
            }
        }
        JsonValue::Array(c) => format!("[{} items]", c.len()),
        JsonValue::Object(e) => format!("{{{} keys}}", e.len()),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub(crate) fn render(
    content: &PreviewContent,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
) {
    if area.height < 3 {
        return;
    }

    // Preview pane with titled border
    let title = match content {
        PreviewContent::Sparkline { count, .. } => format!(" Sparkline \u{2502} {count} values "),
        PreviewContent::Table { total, .. } => format!(" Table \u{2502} {total} rows "),
        PreviewContent::StringList { total, .. } => format!(" Strings \u{2502} {total} items "),
        PreviewContent::KeySummary { entries } => format!(" Object \u{2502} {} keys ", entries.len()),
        PreviewContent::FormattedString { kind, .. } => match kind {
            StringKind::Url => " URL ".into(),
            StringKind::IsoDate => " Date ".into(),
            StringKind::Json => " Embedded JSON ".into(),
            StringKind::Plain => " String ".into(),
        },
        PreviewContent::Scalar { type_name, .. } => format!(" {type_name} "),
    };

    let block = ratatui::widgets::Block::bordered()
        .title(title)
        .title_style(theme.help_title_style)
        .border_style(theme.tree_guide_style)
        .style(theme.bg_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let area = inner;

    match content {
        PreviewContent::Sparkline {
            values,
            min,
            max,
            avg,
            ..
        } => {
            let stats = format!(
                " min: {:.2}  max: {:.2}  avg: {:.2}",
                min, max, avg
            );
            let stats_line = Line::from(Span::styled(stats, theme.fg_dim_style));

            if area.height >= 3 {
                let [stats_area, chart_area] =
                    ratatui::layout::Layout::vertical([
                        ratatui::layout::Constraint::Length(1),
                        ratatui::layout::Constraint::Min(1),
                    ])
                    .areas(area);

                frame.render_widget(
                    ratatui::widgets::Paragraph::new(stats_line).style(theme.bg_style),
                    stats_area,
                );
                frame.render_widget(
                    Sparkline::default()
                        .data(values)
                        .max(100)
                        .style(theme.toolbar_active_style),
                    chart_area,
                );
            } else {
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(stats_line).style(theme.bg_style),
                    area,
                );
            }
        }

        PreviewContent::Table {
            headers,
            rows,
            ..
        } => {
            let mut lines = Vec::new();
            let header_str = format!(" {}", headers.join(" \u{2502} "));
            lines.push(Line::from(Span::styled(
                header_str,
                theme.fg_bold_style.add_modifier(Modifier::UNDERLINED),
            )));

            for row in rows {
                let row_str = format!(" {}", row.join(" \u{2502} "));
                lines.push(Line::from(Span::styled(row_str, theme.fg_style)));
            }

            frame.render_widget(
                ratatui::widgets::Paragraph::new(lines).style(theme.bg_style),
                area,
            );
        }

        PreviewContent::StringList { items, .. } => {
            let mut lines = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let truncated = if item.len() > 80 { crate::util::truncate_chars(item, 77) } else { item };
                lines.push(Line::from(vec![
                    Span::styled(format!(" [{:>3}] ", i), theme.fg_dim_style),
                    Span::styled(truncated, theme.string),
                ]));
            }
            frame.render_widget(
                ratatui::widgets::Paragraph::new(lines).style(theme.bg_style),
                area,
            );
        }

        PreviewContent::KeySummary { entries } => {
            let mut lines = Vec::new();
            for (key, type_name, preview) in entries {
                let value_style = match *type_name {
                    "string" => theme.string,
                    "number" => theme.number,
                    "boolean" => theme.boolean,
                    "null" => theme.null,
                    _ => theme.fg_dim_style,
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {}", key), theme.key),
                    Span::styled(": ", theme.tree_guide_style),
                    Span::styled(preview.as_str(), value_style),
                ]));
            }
            frame.render_widget(
                ratatui::widgets::Paragraph::new(lines).style(theme.bg_style),
                area,
            );
        }

        PreviewContent::FormattedString { text, .. } => {
            let mut lines = vec![Line::from(Span::styled(
                format!(" {} chars", text.len()),
                theme.fg_dim_style,
            ))];

            // Show the text, wrapping if needed
            for line in text.lines().take(area.height as usize - 1) {
                lines.push(Line::from(Span::styled(
                    format!(" {}", line),
                    theme.string,
                )));
            }

            frame.render_widget(
                ratatui::widgets::Paragraph::new(lines).style(theme.bg_style),
                area,
            );
        }

        PreviewContent::Scalar { value, type_name } => {
            let line = Line::from(vec![
                Span::styled(format!(" {} ", type_name), theme.fg_dim_style),
                Span::styled(value.as_str(), theme.fg_bold_style),
            ]);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(line).style(theme.bg_style),
                area,
            );
        }

    }
}
