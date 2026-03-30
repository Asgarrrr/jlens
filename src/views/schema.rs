use std::collections::HashMap;
use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::keymap::Action;
use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;
use crate::util::ScrollState;
use crate::views::{StatusInfo, View, ViewAction};

// ---------------------------------------------------------------------------
// Schema inference
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SchemaNode {
    name: String,
    types: HashMap<&'static str, usize>,
    total_seen: usize,
    children: Vec<SchemaNode>,
}

impl SchemaNode {
    fn dominant_type(&self) -> String {
        let mut types: Vec<_> = self.types.iter().collect();
        types.sort_by(|a, b| b.1.cmp(a.1));
        types
            .iter()
            .map(|(t, _)| **t)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn presence_pct(&self, parent_total: usize) -> u8 {
        if parent_total == 0 {
            return 100;
        }
        ((self.total_seen as f64 / parent_total as f64) * 100.0) as u8
    }
}

fn infer_schema(doc: &JsonDocument, root: NodeId) -> SchemaNode {
    let node = doc.node(root);
    match &node.value {
        JsonValue::Array(children) => {
            let mut schema = SchemaNode {
                name: String::new(),
                types: HashMap::from([("array", 1)]),
                total_seen: 1,
                children: Vec::new(),
            };

            if children.is_empty() {
                return schema;
            }

            // Sample items to infer the array's item schema
            let sample_count = children.len().min(100);
            let item_schema = infer_array_items(doc, &children[..sample_count], children.len());
            schema.children.push(item_schema);
            schema
        }
        JsonValue::Object(entries) => infer_object(doc, entries, 1, 0),
        _ => SchemaNode {
            name: String::new(),
            types: HashMap::from([(node.value.type_name(), 1)]),
            total_seen: 1,
            children: Vec::new(),
        },
    }
}

fn infer_array_items(
    doc: &JsonDocument,
    sample: &[NodeId],
    total: usize,
) -> SchemaNode {
    let sample_count = sample.len();
    let mut merged = SchemaNode {
        name: format!("[item] \u{00d7}{total}"),
        types: HashMap::new(),
        total_seen: sample_count,
        children: Vec::new(),
    };

    // Count types across sampled items
    let mut field_stats: HashMap<String, (HashMap<&'static str, usize>, usize)> = HashMap::new();

    for &id in sample {
        let node = doc.node(id);
        let type_name = node.value.type_name();
        *merged.types.entry(type_name).or_insert(0) += 1;

        if let JsonValue::Object(entries) = &node.value {
            for (key, child_id) in entries {
                let child_node = doc.node(*child_id);
                let child_type = child_node.value.type_name();
                let entry = field_stats
                    .entry(key.to_string())
                    .or_insert_with(|| (HashMap::new(), 0));
                *entry.0.entry(child_type).or_insert(0) += 1;
                entry.1 += 1;
            }
        }
    }

    // Build children from field stats
    let mut children: Vec<SchemaNode> = field_stats
        .into_iter()
        .map(|(name, (types, seen))| {
            // Use sample-relative presence (don't scale — the sample IS representative)
            let scaled_seen = seen.min(sample_count);
            SchemaNode {
                name,
                types,
                total_seen: scaled_seen,
                children: Vec::new(),
            }
        })
        .collect();

    children.sort_by(|a, b| b.total_seen.cmp(&a.total_seen).then(a.name.cmp(&b.name)));
    merged.children = children;
    merged
}

fn infer_object(
    doc: &JsonDocument,
    entries: &[(Arc<str>, NodeId)],
    total: usize,
    depth: u16,
) -> SchemaNode {
    let mut schema = SchemaNode {
        name: String::new(),
        types: HashMap::from([("object", 1)]),
        total_seen: total,
        children: Vec::new(),
    };

    for (key, child_id) in entries {
        let child_node = doc.node(*child_id);
        let child_type = child_node.value.type_name();

        let mut child_schema = SchemaNode {
            name: key.to_string(),
            types: HashMap::from([(child_type, 1)]),
            total_seen: 1,
            children: Vec::new(),
        };

        // Recurse into nested objects/arrays (limited depth)
        if depth < 3 {
            match &child_node.value {
                JsonValue::Object(sub_entries) => {
                    child_schema.children =
                        infer_object(doc, sub_entries, 1, depth + 1).children;
                }
                JsonValue::Array(sub_children) if !sub_children.is_empty() => {
                    let sample = sub_children.len().min(20);
                    let item =
                        infer_array_items(doc, &sub_children[..sample], sub_children.len());
                    child_schema.children.push(item);
                }
                _ => {}
            }
        }

        schema.children.push(child_schema);
    }

    schema
}

// ---------------------------------------------------------------------------
// Flatten schema to displayable rows
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SchemaRow {
    indent: u16,
    name: String,
    type_str: String,
    presence: String,
    is_container: bool,
}

fn flatten_schema(schema: &SchemaNode, parent_total: usize) -> Vec<SchemaRow> {
    let mut rows = Vec::new();
    flatten_node(schema, parent_total, 0, &mut rows);
    rows
}

fn flatten_node(
    node: &SchemaNode,
    parent_total: usize,
    indent: u16,
    rows: &mut Vec<SchemaRow>,
) {
    let type_str = node.dominant_type();
    let pct = node.presence_pct(parent_total);
    let presence = if pct == 100 {
        String::new()
    } else {
        format!("{}%", pct)
    };

    let is_container = !node.children.is_empty();

    if !node.name.is_empty() {
        rows.push(SchemaRow {
            indent,
            name: node.name.clone(),
            type_str,
            presence,
            is_container,
        });
    } else if indent == 0 {
        // Root node
        let label = if node.types.contains_key("array") {
            format!("root (array[{}])", node.total_seen)
        } else {
            format!("root ({})", node.dominant_type())
        };
        rows.push(SchemaRow {
            indent: 0,
            name: label,
            type_str: String::new(),
            presence: String::new(),
            is_container: true,
        });
    }

    for child in &node.children {
        flatten_node(child, node.total_seen, indent + 1, rows);
    }
}

// ---------------------------------------------------------------------------
// SchemaView
// ---------------------------------------------------------------------------

pub struct SchemaView {
    rows: Vec<SchemaRow>,
    scroll: ScrollState,
}

impl SchemaView {
    pub fn new(document: Arc<JsonDocument>, root: NodeId) -> Self {
        let schema = infer_schema(&document, root);
        let rows = flatten_schema(&schema, 1);
        Self {
            rows,
            scroll: ScrollState::new(),
        }
    }
}

impl View for SchemaView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let height = area.height as usize;
        let start = self.scroll.offset;
        let end = (start + height).min(self.rows.len());

        let lines: Vec<Line> = (start..end)
            .map(|i| {
                let row = &self.rows[i];
                let is_selected = i == self.scroll.selected;

                let indent_str = "  ".repeat(row.indent as usize);
                let connector = if row.indent > 0 { "\u{251c}\u{2500} " } else { "" };

                let mut spans = vec![
                    Span::styled(
                        format!("{indent_str}{connector}"),
                        theme.tree_guide_style,
                    ),
                    Span::styled(
                        row.name.as_str(),
                        if row.is_container {
                            theme.fg_bold_style
                        } else {
                            theme.key
                        },
                    ),
                ];

                if !row.type_str.is_empty() {
                    // Pad to align types
                    let name_width = row.indent as usize * 2
                        + connector.len()
                        + crate::util::display_width(&row.name);
                    let pad = 40usize.saturating_sub(name_width);
                    spans.push(Span::raw(" ".repeat(pad)));

                    // Color type based on name
                    let type_style = match row.type_str.as_str() {
                        "string" => theme.string,
                        "number" => theme.number,
                        "bool" => theme.boolean,
                        "null" => theme.null,
                        "object" => theme.bracket,
                        "array" => theme.bracket,
                        _ => theme.fg_dim_style, // mixed types
                    };
                    spans.push(Span::styled(row.type_str.as_str(), type_style));
                }

                if !row.presence.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", row.presence),
                        theme.fg_dim_style,
                    ));
                }

                let style = if is_selected {
                    theme.selection_style
                } else {
                    theme.bg_style
                };

                Line::from(spans).style(style)
            })
            .collect();

        frame.render_widget(
            ratatui::widgets::Paragraph::new(lines).style(theme.bg_style),
            area,
        );

        if self.rows.len() > height {
            crate::ui::render_scrollbar(frame, area, self.rows.len(), self.scroll.offset, theme);
        }
    }

    fn handle_action(&mut self, action: Action) -> ViewAction {
        match action {
            Action::MoveUp => self.scroll.move_up(),
            Action::MoveDown => self.scroll.move_down(self.rows.len()),
            Action::PageUp => self.scroll.page_up(2),
            Action::PageDown => self.scroll.page_down(self.rows.len(), 2),
            Action::Home => self.scroll.go_top(),
            Action::End => self.scroll.go_bottom(self.rows.len()),
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        StatusInfo {
            cursor_path: format!("schema ({} fields)", self.rows.len()),
        }
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.scroll.viewport = height;
        self.scroll.clamp(self.rows.len());
    }
}
