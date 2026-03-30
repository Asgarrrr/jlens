use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use ratatui::Frame;

use crate::model::node::{JsonDocument, JsonValue, NodeId};
use crate::theme::Theme;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub(crate) struct FinderState {
    pub active: bool,
    query: String,
    paths: Vec<(String, NodeId, String)>, // (path, node_id, value_preview)
    results: Vec<(usize, i32)>,           // (index into paths, score)
    selected: usize,
}

pub(crate) enum FinderAction {
    None,
    Close,
    Jump(NodeId),
}

impl FinderState {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            paths: Vec::new(),
            results: Vec::new(),
            selected: 0,
        }
    }

    pub fn open(&mut self, doc: &JsonDocument, root: NodeId) {
        self.active = true;
        self.query.clear();
        self.selected = 0;
        self.paths = collect_paths(doc, root);
        self.results = (0..self.paths.len()).map(|i| (i, 0)).collect();
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FinderAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => FinderAction::Close,
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if let Some(&(idx, _)) = self.results.get(self.selected) {
                    FinderAction::Jump(self.paths[idx].1)
                } else {
                    FinderAction::Close
                }
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                FinderAction::None
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                if self.selected + 1 < self.results.len().min(100) {
                    self.selected += 1;
                }
                FinderAction::None
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.query.pop();
                self.refilter();
                FinderAction::None
            }
            (mods, KeyCode::Char(c))
                if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if self.query.len() < 256 {
                    self.query.push(c);
                    self.refilter();
                }
                FinderAction::None
            }
            _ => FinderAction::None,
        }
    }

    fn refilter(&mut self) {
        self.selected = 0;
        if self.query.is_empty() {
            self.results = (0..self.paths.len()).map(|i| (i, 0)).collect();
            return;
        }

        let query = self.query.to_lowercase();
        let mut scored: Vec<(usize, i32)> = self
            .paths
            .iter()
            .enumerate()
            .filter_map(|(i, (path, _, val))| {
                // Match against path and value
                let path_score = fuzzy_score(&query, &path.to_lowercase());
                let val_score = fuzzy_score(&query, &val.to_lowercase());
                let best = match (path_score, val_score) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };
                best.map(|s| (i, s))
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        self.results = scored;
    }
}

// ---------------------------------------------------------------------------
// Path collection
// ---------------------------------------------------------------------------

fn collect_paths(doc: &JsonDocument, root: NodeId) -> Vec<(String, NodeId, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<(NodeId, String)> = vec![(root, "$".into())];

    while let Some((id, path)) = stack.pop() {
        let node = doc.node(id);
        match &node.value {
            JsonValue::Object(entries) => {
                // Add the object itself
                out.push((path.clone(), id, format!("{{{} keys}}", entries.len())));
                for (key, child_id) in entries.iter().rev() {
                    stack.push((*child_id, format!("{}.{}", path, key)));
                }
            }
            JsonValue::Array(children) => {
                out.push((path.clone(), id, format!("[{} items]", children.len())));
                for (i, child_id) in children.iter().enumerate().rev() {
                    stack.push((*child_id, format!("{}[{}]", path, i)));
                }
            }
            _ => {
                let preview = value_preview(node);
                out.push((path, id, preview));
            }
        }
    }

    out
}

fn value_preview(node: &crate::model::node::JsonNode) -> String {
    match &node.value {
        JsonValue::Null => "null".into(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => {
            if s.len() > 30 {
                format!("\"{}...\"", crate::util::truncate_chars(s, 27))
            } else {
                format!("\"{}\"", s)
            }
        }
        JsonValue::Array(c) => format!("[{} items]", c.len()),
        JsonValue::Object(e) => format!("{{{} keys}}", e.len()),
    }
}

// ---------------------------------------------------------------------------
// Fuzzy scoring
// ---------------------------------------------------------------------------

/// Subsequence match with scoring. Returns None if query is not a subsequence of target.
fn fuzzy_score(query: &str, target: &str) -> Option<i32> {
    let query_chars: Vec<char> = query.chars().collect();
    if query_chars.is_empty() {
        return Some(0);
    }

    let target_chars: Vec<char> = target.chars().collect();
    let mut qi = 0;
    let mut score = 0i32;
    let mut prev_match = false;

    for (ti, &tc) in target_chars.iter().enumerate() {
        if qi < query_chars.len() && tc == query_chars[qi] {
            score += 1;
            // Consecutive match bonus
            if prev_match {
                score += 3;
            }
            // Start-of-word bonus (after '.', '[', '_', or at position 0)
            if ti == 0 || matches!(target_chars.get(ti - 1), Some('.' | '[' | '_' | '/')) {
                score += 5;
            }
            qi += 1;
            prev_match = true;
        } else {
            prev_match = false;
        }
    }

    if qi == query_chars.len() {
        Some(score)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub(crate) fn render_overlay(
    frame: &mut Frame,
    state: &FinderState,
    theme: &Theme,
) {
    let area = frame.area();
    // 80% width, 60% height, centered
    let w = (area.width * 80 / 100).max(40).min(area.width);
    let h = (area.height * 60 / 100).max(10).min(area.height);
    let x = (area.width - w) / 2;
    let y = (area.height - h) / 2;
    let overlay = Rect::new(x, y, w, h);

    // Clear background
    frame.render_widget(Clear, overlay);

    let block = Block::bordered()
        .title(" Find path ")
        .title_style(theme.help_title_style)
        .border_style(theme.help_border_style)
        .style(theme.bg_style);

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.height < 3 {
        return;
    }

    // Input line
    let input_area = Rect::new(inner.x, inner.y, inner.width, 1);
    let input_line = Line::from(vec![
        Span::styled(" > ", theme.toolbar_brand_style),
        Span::styled(state.query.as_str(), theme.fg_style),
        Span::styled("\u{2588}", theme.input_cursor_style),
        Span::styled(
            format!("  {} results", state.results.len().min(100)),
            theme.fg_dim_style,
        ),
    ]);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(input_line).style(theme.bg_style),
        input_area,
    );

    // Separator
    let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
    let sep = Line::from(Span::styled(
        "\u{2500}".repeat(inner.width as usize),
        theme.tree_guide_style,
    ));
    frame.render_widget(ratatui::widgets::Paragraph::new(sep), sep_area);

    // Results
    let results_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height - 2);
    let max_results = results_area.height as usize;
    let mut lines = Vec::new();

    for (i, &(idx, _score)) in state.results.iter().take(max_results).enumerate() {
        let (path, _, value) = &state.paths[idx];
        let is_selected = i == state.selected;

        let path_display = if path.len() as u16 > inner.width / 2 {
            let max = (inner.width / 2 - 3) as usize;
            let skip = path.char_indices()
                .rev()
                .nth(max.min(path.chars().count()))
                .map(|(i, _)| i)
                .unwrap_or(0);
            format!("...{}", &path[skip..])
        } else {
            path.clone()
        };

        let val_width = inner.width as usize - crate::util::display_width(&path_display) - 3;
        let val_display = if value.len() > val_width {
            format!("{}...", crate::util::truncate_chars(value, val_width.saturating_sub(3)))
        } else {
            value.clone()
        };

        let style = if is_selected {
            theme.selection_style
        } else {
            theme.bg_style
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", path_display),
                if is_selected {
                    theme.selection_style.add_modifier(Modifier::BOLD)
                } else {
                    theme.key
                },
            ),
            Span::styled(val_display, if is_selected { style } else { theme.fg_dim_style }),
        ]).style(style));
    }

    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines).style(theme.bg_style),
        results_area,
    );
}
