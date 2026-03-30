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
// Layout — compute (x, y) position for each node
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GraphNode {
    x: f64,
    y: f64,
    label: String,
    color: Color,
    width: f64,
    children: Vec<usize>, // indices into the nodes vec
}

struct GraphLayout {
    nodes: Vec<GraphNode>,
}

impl GraphLayout {
    fn build(doc: &JsonDocument, root: NodeId, max_depth: u16) -> Self {
        let mut nodes = Vec::new();
        let mut stack: Vec<(NodeId, u16, Option<usize>)> = vec![(root, 0, None)];
        let mut parent_children: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();

        while let Some((id, depth, parent_idx)) = stack.pop() {
            if depth > max_depth {
                continue;
            }

            let node = doc.node(id);
            let (label, color) = node_label_color(node, id, depth);

            let idx = nodes.len();
            nodes.push(GraphNode {
                x: 0.0,
                y: -(depth as f64) * 4.0,
                label,
                color,
                width: 0.0,
                children: Vec::new(),
            });

            if let Some(pi) = parent_idx {
                parent_children.entry(pi).or_default().push(idx);
            }

            // Push children (limited to first N per container)
            let max_children = match depth {
                0 => 20,
                1 => 10,
                2 => 5,
                _ => 3,
            };

            match &node.value {
                JsonValue::Object(entries) => {
                    for (_, child_id) in entries.iter().take(max_children).rev() {
                        stack.push((*child_id, depth + 1, Some(idx)));
                    }
                }
                JsonValue::Array(children) => {
                    for &child_id in children.iter().take(max_children).rev() {
                        stack.push((child_id, depth + 1, Some(idx)));
                    }
                }
                _ => {}
            }
        }

        // Wire children
        for (parent_idx, children) in &parent_children {
            nodes[*parent_idx].children = children.clone();
        }

        // Compute x positions (simple: spread children evenly)
        let mut layout = Self { nodes };
        if !layout.nodes.is_empty() {
            layout.compute_x(0, 0.0);
        }
        layout
    }

    fn compute_x(&mut self, idx: usize, min_x: f64) -> f64 {
        let children = self.nodes[idx].children.clone();
        if children.is_empty() {
            let w = self.nodes[idx].label.len().max(4) as f64 + 2.0;
            self.nodes[idx].x = min_x + w / 2.0;
            self.nodes[idx].width = w;
            return w;
        }

        let mut total_width = 0.0;
        let gap = 2.0;
        for (i, &child_idx) in children.iter().enumerate() {
            let child_w = self.compute_x(child_idx, min_x + total_width);
            total_width += child_w;
            if i < children.len() - 1 {
                total_width += gap;
            }
        }

        let w = total_width.max(self.nodes[idx].label.len() as f64 + 2.0);
        self.nodes[idx].x = min_x + w / 2.0;
        self.nodes[idx].width = w;
        w
    }
}

fn node_label_color(
    node: &crate::model::node::JsonNode,
    _id: NodeId,
    _depth: u16,
) -> (String, Color) {
    match &node.value {
        JsonValue::Null => ("null".into(), Color::Rgb(108, 112, 134)),
        JsonValue::Bool(b) => (b.to_string(), Color::Rgb(203, 166, 247)),
        JsonValue::Number(n) => (n.to_string(), Color::Rgb(250, 179, 135)),
        JsonValue::String(s) => {
            let display = if s.len() > 12 {
                format!("\"{}..\"", &s[..s.char_indices().nth(10).map(|(i, _)| i).unwrap_or(s.len())])
            } else {
                format!("\"{s}\"")
            };
            (display, Color::Rgb(166, 227, 161))
        }
        JsonValue::Object(entries) => {
            (format!("{{{}}}", entries.len()), Color::Rgb(137, 180, 250))
        }
        JsonValue::Array(children) => {
            (format!("[{}]", children.len()), Color::Rgb(137, 180, 250))
        }
    }
}

// ---------------------------------------------------------------------------
// GraphView
// ---------------------------------------------------------------------------

pub struct GraphView {
    layout: GraphLayout,
    // Viewport (pan + zoom)
    center_x: f64,
    center_y: f64,
    zoom: f64,
    selected: usize,
}

impl GraphView {
    pub fn new(document: Arc<JsonDocument>, root: NodeId) -> Self {
        let layout = GraphLayout::build(&document, root, 4);
        Self {
            layout,
            center_x: 0.0,
            center_y: 0.0,
            zoom: 1.0,
            selected: 0,
        }
    }

    fn pan(&mut self, dx: f64, dy: f64) {
        self.center_x += dx / self.zoom;
        self.center_y += dy / self.zoom;
    }

    fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.3).min(5.0);
    }

    fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.3).max(0.2);
    }

    fn select_next(&mut self) {
        if self.selected + 1 < self.layout.nodes.len() {
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
        if let Some(node) = self.layout.nodes.get(self.selected) {
            self.center_x = node.x;
            self.center_y = node.y;
        }
    }
}

impl View for GraphView {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let half_w = (area.width as f64) / self.zoom / 2.0;
        let half_h = (area.height as f64 * 2.0) / self.zoom / 2.0; // braille = 2x height

        let x_min = self.center_x - half_w;
        let x_max = self.center_x + half_w;
        let y_min = self.center_y - half_h;
        let y_max = self.center_y + half_h;

        let canvas = Canvas::default()
            .x_bounds([x_min, x_max])
            .y_bounds([y_min, y_max])
            .marker(ratatui::symbols::Marker::Braille)
            .background_color(theme.bg)
            .paint(|ctx| {
                // Draw edges
                for node in &self.layout.nodes {
                    for &child_idx in &node.children {
                        let child = &self.layout.nodes[child_idx];
                        ctx.draw(&CanvasLine {
                            x1: node.x,
                            y1: node.y - 0.5,
                            x2: child.x,
                            y2: child.y + 1.5,
                            color: Color::Rgb(88, 91, 112),
                        });
                    }
                }

                // Draw nodes
                for (i, node) in self.layout.nodes.iter().enumerate() {
                    let is_selected = i == self.selected;
                    let half_w = node.label.len() as f64 / 2.0 + 0.5;

                    // Node box
                    let border_color = if is_selected {
                        Color::Rgb(249, 226, 175)
                    } else {
                        node.color
                    };

                    ctx.draw(&Rectangle {
                        x: node.x - half_w,
                        y: node.y - 0.5,
                        width: half_w * 2.0,
                        height: 2.0,
                        color: border_color,
                    });

                    // Label — clone the label so it has 'static lifetime
                    let label: String = node.label.clone();
                    let node_color = node.color;
                    let label_x = node.x - half_w + 0.5;
                    let label_y = node.y + 0.5;
                    ctx.print(
                        label_x,
                        label_y,
                        ratatui::text::Span::styled(
                            label,
                            ratatui::style::Style::new().fg(node_color),
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
            Action::ExpandNode => self.pan(5.0, 0.0),
            Action::CollapseNode => self.pan(-5.0, 0.0),
            Action::Home => self.zoom_in(),
            Action::End => self.zoom_out(),
            Action::ToggleExpand => self.center_on_selected(),
            Action::CopyValue => {
                self.select_next();
            }
            Action::CopyPath => {
                self.select_prev();
            }
            _ => {}
        }
        ViewAction::None
    }

    fn status_info(&self) -> StatusInfo {
        let label = self
            .layout
            .nodes
            .get(self.selected)
            .map(|n| n.label.as_str())
            .unwrap_or("?");
        StatusInfo {
            cursor_path: format!("graph ({} nodes) zoom:{:.0}% | {}", self.layout.nodes.len(), self.zoom * 100.0, label),
        }
    }

    fn set_viewport_height(&mut self, _height: usize) {}
}
