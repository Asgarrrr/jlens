use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Rectangle};
use ratatui::Frame;

use crate::keymap::Action;
use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;
use crate::views::{StatusInfo, View, ViewAction};

// ---------------------------------------------------------------------------
// Graph node layout
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GNode {
    x: f64,
    y: f64,
    label: String,
    color: Color,
    width: f64,
    children: Vec<usize>,
}

/// Compute a top-down tree layout from the JSON document.
fn build_layout(doc: &JsonDocument, root: NodeId, max_depth: u16) -> Vec<GNode> {
    let mut nodes = Vec::new();
    let mut queue: Vec<(NodeId, u16, Option<usize>)> = vec![(root, 0, None)];

    // BFS to build flat list
    while let Some((id, depth, parent_idx)) = queue.pop() {
        if depth > max_depth {
            continue;
        }

        let node = doc.node(id);
        let (label, color) = make_label(node);
        let max_kids = if depth < 2 { 8 } else { 4 };

        let idx = nodes.len();
        nodes.push(GNode {
            x: 0.0,
            y: -(depth as f64) * 6.0, // vertical spacing between levels
            label,
            color,
            width: 0.0,
            children: Vec::new(),
        });

        if let Some(pi) = parent_idx {
            nodes[pi].children.push(idx);
        }

        match &node.value {
            JsonValue::Object(entries) => {
                let count = entries.len();
                for (key, child_id) in entries.iter().take(max_kids).rev() {
                    let child_node = doc.node(*child_id);
                    let child_label = format!("{}: {}", key, short_value(child_node));
                    let child_color = value_color(child_node);
                    let child_idx = nodes.len();
                    nodes.push(GNode {
                        x: 0.0,
                        y: -((depth + 1) as f64) * 6.0,
                        label: child_label,
                        color: child_color,
                        width: 0.0,
                        children: Vec::new(),
                    });
                    nodes[idx].children.push(child_idx);
                    // Don't recurse into leaves — only containers get children
                    if matches!(child_node.value, JsonValue::Object(_) | JsonValue::Array(_))
                        && depth + 1 < max_depth
                    {
                        queue.push((*child_id, depth + 2, Some(child_idx)));
                    }
                }
                if count > max_kids {
                    let more_idx = nodes.len();
                    nodes.push(GNode {
                        x: 0.0,
                        y: -((depth + 1) as f64) * 6.0,
                        label: format!("+{} more", count - max_kids),
                        color: Color::Rgb(108, 112, 134),
                        width: 0.0,
                        children: Vec::new(),
                    });
                    nodes[idx].children.push(more_idx);
                }
            }
            JsonValue::Array(children) => {
                let count = children.len();
                for &child_id in children.iter().take(max_kids).rev() {
                    queue.push((child_id, depth + 1, Some(idx)));
                }
                if count > max_kids {
                    let more_idx = nodes.len();
                    nodes.push(GNode {
                        x: 0.0,
                        y: -((depth + 1) as f64) * 6.0,
                        label: format!("+{} more", count - max_kids),
                        color: Color::Rgb(108, 112, 134),
                        width: 0.0,
                        children: Vec::new(),
                    });
                    nodes[idx].children.push(more_idx);
                }
            }
            _ => {}
        }
    }

    // Compute x positions: each leaf gets a slot, parents center over children
    if !nodes.is_empty() {
        assign_x(&mut nodes, 0, 0.0);
    }
    nodes
}

/// Assign x positions bottom-up. Returns the width consumed.
fn assign_x(nodes: &mut Vec<GNode>, idx: usize, min_x: f64) -> f64 {
    let children = nodes[idx].children.clone();
    let w = (nodes[idx].label.len() as f64).max(6.0) + 2.0;

    if children.is_empty() {
        nodes[idx].x = min_x + w / 2.0;
        nodes[idx].width = w;
        return w;
    }

    let gap = 3.0;
    let mut total = 0.0;
    for (i, &child) in children.iter().enumerate() {
        let child_w = assign_x(nodes, child, min_x + total);
        total += child_w;
        if i < children.len() - 1 {
            total += gap;
        }
    }

    let total = total.max(w);
    nodes[idx].x = min_x + total / 2.0;
    nodes[idx].width = total;
    total
}

fn make_label(node: &crate::model::node::JsonNode) -> (String, Color) {
    match &node.value {
        JsonValue::Object(e) => (format!("{{{}}} ", e.len()), Color::Rgb(137, 180, 250)),
        JsonValue::Array(c) => (format!("[{}]", c.len()), Color::Rgb(137, 180, 250)),
        _ => (short_value(node), value_color(node)),
    }
}

fn short_value(node: &crate::model::node::JsonNode) -> String {
    match &node.value {
        JsonValue::Null => "null".into(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => {
            if s.len() > 15 {
                let end = s.char_indices().nth(13).map(|(i, _)| i).unwrap_or(s.len());
                format!("\"{}..\"", &s[..end])
            } else {
                format!("\"{s}\"")
            }
        }
        JsonValue::Object(e) => format!("{{{}}}", e.len()),
        JsonValue::Array(c) => format!("[{}]", c.len()),
    }
}

fn value_color(node: &crate::model::node::JsonNode) -> Color {
    match &node.value {
        JsonValue::Null => Color::Rgb(108, 112, 134),
        JsonValue::Bool(_) => Color::Rgb(203, 166, 247),
        JsonValue::Number(_) => Color::Rgb(250, 179, 135),
        JsonValue::String(_) => Color::Rgb(166, 227, 161),
        JsonValue::Object(_) | JsonValue::Array(_) => Color::Rgb(137, 180, 250),
    }
}

// ---------------------------------------------------------------------------
// GraphView
// ---------------------------------------------------------------------------

pub struct GraphView {
    nodes: Vec<GNode>,
    center_x: f64,
    center_y: f64,
    zoom: f64,
    selected: usize,
    /// For mouse drag: last mouse position
    drag_from: Option<(u16, u16)>,
}

impl GraphView {
    pub fn new(document: Arc<JsonDocument>, root: NodeId) -> Self {
        let nodes = build_layout(&document, root, 3);
        let cx = nodes.first().map(|n| n.x).unwrap_or(0.0);
        let cy = nodes.first().map(|n| n.y).unwrap_or(0.0);
        Self {
            nodes,
            center_x: cx,
            center_y: cy,
            zoom: 1.0,
            selected: 0,
            drag_from: None,
        }
    }

    fn pan(&mut self, dx: f64, dy: f64) {
        self.center_x += dx / self.zoom;
        self.center_y += dy / self.zoom;
    }

    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.3).min(5.0);
    }

    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.3).max(0.2);
    }

    fn select_next(&mut self) {
        if self.selected + 1 < self.nodes.len() {
            self.selected += 1;
            self.center_on_selected();
        }
    }

    fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.center_on_selected();
        }
    }

    fn center_on_selected(&mut self) {
        if let Some(node) = self.nodes.get(self.selected) {
            self.center_x = node.x;
            self.center_y = node.y;
        }
    }

    /// Handle mouse drag for panning.
    pub fn handle_mouse_drag(&mut self, x: u16, y: u16) {
        if let Some((prev_x, prev_y)) = self.drag_from {
            let dx = -(x as f64 - prev_x as f64);
            let dy = (y as f64 - prev_y as f64) * 1.0;
            self.pan(dx, dy);
        }
        self.drag_from = Some((x, y));
    }

    pub fn handle_mouse_release(&mut self) {
        self.drag_from = None;
    }
}

impl View for GraphView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let half_w = (area.width as f64) / self.zoom;
        let half_h = (area.height as f64 * 2.0) / self.zoom;

        let x_min = self.center_x - half_w / 2.0;
        let x_max = self.center_x + half_w / 2.0;
        let y_min = self.center_y - half_h / 2.0;
        let y_max = self.center_y + half_h / 2.0;

        let canvas = Canvas::default()
            .x_bounds([x_min, x_max])
            .y_bounds([y_min, y_max])
            .marker(ratatui::symbols::Marker::HalfBlock)
            .background_color(theme.bg)
            .paint(|ctx| {
                // Draw edges first (behind nodes)
                for node in &self.nodes {
                    for &child_idx in &node.children {
                        let child = &self.nodes[child_idx];
                        ctx.draw(&CanvasLine {
                            x1: node.x,
                            y1: node.y - 1.0,
                            x2: child.x,
                            y2: child.y + 2.0,
                            color: Color::Rgb(88, 91, 112),
                        });
                    }
                }

                ctx.layer();

                // Draw nodes
                for (i, node) in self.nodes.iter().enumerate() {
                    let is_sel = i == self.selected;
                    let half_w = (node.label.len() as f64 / 2.0).max(3.0);

                    let border = if is_sel {
                        Color::Rgb(249, 226, 175)
                    } else {
                        node.color
                    };

                    ctx.draw(&Rectangle {
                        x: node.x - half_w,
                        y: node.y - 1.0,
                        width: half_w * 2.0,
                        height: 3.0,
                        color: border,
                    });

                    let label = node.label.clone();
                    let color = node.color;
                    ctx.print(
                        node.x - half_w + 0.5,
                        node.y,
                        ratatui::text::Span::styled(
                            label,
                            ratatui::style::Style::new().fg(color),
                        ),
                    );
                }
            });

        frame.render_widget(canvas, area);
    }

    fn handle_action(&mut self, action: Action) -> ViewAction {
        match action {
            Action::MoveUp => self.pan(0.0, 3.0),
            Action::MoveDown => self.pan(0.0, -3.0),
            Action::PageUp => self.pan(0.0, 10.0),
            Action::PageDown => self.pan(0.0, -10.0),
            Action::ExpandNode | Action::ScrollRight => self.pan(5.0, 0.0),
            Action::CollapseNode | Action::ScrollLeft => self.pan(-5.0, 0.0),
            Action::PreviewGrow => self.zoom_in(),
            Action::PreviewShrink => self.zoom_out(),
            Action::Home => { self.zoom = 1.0; self.center_on_selected(); }
            Action::NextSearchHit => self.select_next(),
            Action::PrevSearchHit => self.select_prev(),
            Action::ToggleExpand => self.center_on_selected(),
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        let label = self.nodes.get(self.selected).map(|n| n.label.as_str()).unwrap_or("?");
        StatusInfo {
            cursor_path: format!(
                "graph | {} nodes | zoom {:.0}% | {}",
                self.nodes.len(),
                self.zoom * 100.0,
                label,
            ),
        }
    }

    fn set_viewport_height(&mut self, _height: usize) {}
}
