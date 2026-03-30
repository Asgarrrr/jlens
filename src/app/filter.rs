use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::model::node::{DocumentBuilder, JsonDocument};
use crate::theme::Theme;
use crate::views::View;
use crate::views::raw;
use crate::views::tree::TreeView;

const DEBOUNCE: Duration = Duration::from_millis(50); // fast feedback

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub(crate) struct FilterResult {
    pub view: TreeView,
}

pub(crate) struct FilterState {
    pub(crate) active: bool,
    pub(crate) query: String,
    pub(crate) cursor: usize,

    /// The evaluated result (live during input, persists after Enter).
    pub(crate) result: Option<FilterResult>,
    pub(crate) error: Option<String>,
    pub(crate) count: usize,

    // Debounce: don't re-evaluate on every keystroke.
    last_edit: Instant,
    needs_eval: bool,

    // Autocomplete
    pub(crate) suggestions: Vec<String>,
    pub(crate) suggestion_idx: usize,
    pub(crate) show_suggestions: bool,

    // History
    pub(crate) history: Vec<String>,
    history_idx: Option<usize>,
    history_draft: String,
}

pub(crate) enum FilterAction {
    None,
    Close,
    Apply,
    Reopen,
    DelegateToResult(KeyEvent),
}

impl FilterState {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            cursor: 0,
            result: None,
            error: None,
            count: 0,
            last_edit: Instant::now(),
            needs_eval: false,
            suggestions: Vec::new(),
            suggestion_idx: 0,
            show_suggestions: false,
            history: Vec::new(),
            history_idx: None,
            history_draft: String::new(),
        }
    }

    pub(crate) fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.cursor = 0;
        self.error = None;
        self.result = None;
        self.count = 0;
        self.needs_eval = false;
        self.suggestions.clear();
        self.show_suggestions = false;
        self.history_idx = None;
        self.history_draft.clear();
    }

    pub(crate) fn reopen(&mut self) {
        self.active = true;
        self.cursor = self.query.len();
        self.error = None;
        self.show_suggestions = false;
        self.history_idx = None;
    }

    pub(crate) fn close(&mut self) {
        self.active = false;
    }

    pub(crate) fn clear_result(&mut self) {
        self.result = None;
        self.error = None;
        self.count = 0;
    }

    pub(crate) fn has_result(&self) -> bool {
        self.result.is_some()
    }

    /// Should the eval run now? (debounce check)
    pub(crate) fn should_eval(&self) -> bool {
        self.needs_eval && self.last_edit.elapsed() >= DEBOUNCE
    }

    fn mark_edited(&mut self) {
        self.last_edit = Instant::now();
        self.needs_eval = true;
        self.error = None;
    }

    // -------------------------------------------------------------------
    // Key handling
    // -------------------------------------------------------------------

    pub(crate) fn handle_input_key(&mut self, key: KeyEvent) -> FilterAction {
        match (key.modifiers, key.code) {
            // Esc always closes — one keystroke, no ambiguity
            (KeyModifiers::NONE, KeyCode::Esc) => FilterAction::Close,

            // Enter: accept suggestion if visible, otherwise apply filter
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if self.show_suggestions && !self.suggestions.is_empty() {
                    self.accept_suggestion();
                    self.mark_edited();
                    FilterAction::None
                } else {
                    let q = self.query.trim().to_string();
                    if !q.is_empty() {
                        self.history.retain(|h| h != &q);
                        self.history.push(q);
                    }
                    FilterAction::Apply
                }
            }

            // Tab: accept current suggestion or cycle if already accepted
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if self.show_suggestions && !self.suggestions.is_empty() {
                    self.accept_suggestion();
                    self.mark_edited();
                }
                FilterAction::None
            }

            // Down/Up: navigate suggestions if visible, otherwise history
            (KeyModifiers::NONE, KeyCode::Down) => {
                if self.show_suggestions && !self.suggestions.is_empty() {
                    self.suggestion_idx = (self.suggestion_idx + 1) % self.suggestions.len();
                    return FilterAction::None;
                }
                // History: forward
                if let Some(idx) = self.history_idx {
                    if idx + 1 < self.history.len() {
                        self.history_idx = Some(idx + 1);
                        self.query = self.history[idx + 1].clone();
                    } else {
                        self.history_idx = None;
                        self.query = self.history_draft.clone();
                    }
                    self.cursor = self.query.len();
                    self.mark_edited();
                }
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.show_suggestions && !self.suggestions.is_empty() {
                    self.suggestion_idx = self.suggestion_idx.saturating_sub(1);
                    return FilterAction::None;
                }
                // History: backward
                if !self.history.is_empty() {
                    match self.history_idx {
                        None => {
                            self.history_draft = self.query.clone();
                            self.history_idx = Some(self.history.len() - 1);
                        }
                        Some(idx) if idx > 0 => {
                            self.history_idx = Some(idx - 1);
                        }
                        _ => {}
                    }
                    if let Some(idx) = self.history_idx {
                        self.query = self.history[idx].clone();
                        self.cursor = self.query.len();
                        self.mark_edited();
                    }
                }
                FilterAction::None
            }

            // Cursor
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.cursor > 0 {
                    self.cursor = self.query[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                if self.cursor < self.query.len() {
                    self.cursor = self.query[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor + i)
                        .unwrap_or(self.query.len());
                }
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Home) | (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.cursor = 0;
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.cursor = self.query.len();
                FilterAction::None
            }

            // Editing
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if self.cursor > 0 {
                    let prev = self.query[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.query.drain(prev..self.cursor);
                    self.cursor = prev;
                    self.mark_edited();
                }
                self.show_suggestions = false;
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Delete) => {
                if self.cursor < self.query.len() {
                    let next = self.query[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor + i)
                        .unwrap_or(self.query.len());
                    self.query.drain(self.cursor..next);
                    self.mark_edited();
                }
                FilterAction::None
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.query.drain(..self.cursor);
                self.cursor = 0;
                self.mark_edited();
                self.show_suggestions = false;
                FilterAction::None
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                // Delete word backwards (like readline)
                if self.cursor > 0 {
                    let text = &self.query[..self.cursor];
                    let trimmed = text.trim_end();
                    let word_start = trimmed
                        .rfind([' ', '.', '|', '('])
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    self.query.drain(word_start..self.cursor);
                    self.cursor = word_start;
                    self.mark_edited();
                    self.show_suggestions = false;
                }
                FilterAction::None
            }
            (mods, KeyCode::Char(c))
                if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if self.query.len() < 1024 {
                    self.query.insert(self.cursor, c);
                    self.cursor += c.len_utf8();
                    self.mark_edited();
                    // Auto-show suggestions after . or | (the most common trigger points)
                    self.show_suggestions = matches!(c, '.' | '|');
                }
                FilterAction::None
            }
            _ => FilterAction::None,
        }
    }

    fn accept_suggestion(&mut self) {
        let Some(suggestion) = self.suggestions.get(self.suggestion_idx).cloned() else {
            return;
        };
        let query_to_cursor = &self.query[..self.cursor];
        let prefix = completion_prefix(query_to_cursor);
        let prefix_start = self.cursor - prefix.len();
        self.query.replace_range(prefix_start..self.cursor, &suggestion);
        self.cursor = prefix_start + suggestion.len();
        self.suggestion_idx = 0;
        // Keep suggestions open if the accepted suggestion ends with something
        // that naturally leads to more input (like a field name → might chain with .)
        self.show_suggestions = false;
    }

    pub(crate) fn handle_result_key(&mut self, key: KeyEvent) -> FilterAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => FilterAction::Close,
            (KeyModifiers::NONE, KeyCode::Char(':')) => FilterAction::Reopen,
            _ => FilterAction::DelegateToResult(key),
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation (single path — used by both live preview and Enter)
// ---------------------------------------------------------------------------

pub(crate) fn evaluate(
    filter: &mut FilterState,
    document: &JsonDocument,
    cached_value: &mut Option<serde_json::Value>,
    viewport_height: usize,
) {
    filter.needs_eval = false;

    let query = filter.query.trim();
    if query.is_empty() {
        filter.result = None;
        filter.error = None;
        filter.count = 0;
        return;
    }

    let expr = match crate::filter::parse::parse(query) {
        Ok(e) => e,
        Err(e) => {
            filter.error = Some(e.to_string());
            filter.result = None;
            filter.count = 0;
            return;
        }
    };

    let root_value = cached_value
        .get_or_insert_with(|| raw::rebuild_serde_value(document, document.root()));

    match crate::filter::eval::apply(root_value, &expr) {
        Ok(results) => {
            filter.count = results.len();
            filter.error = None;

            if results.is_empty() {
                filter.result = None;
                return;
            }

            let value = if results.len() == 1 {
                results.into_iter().next().unwrap()
            } else {
                serde_json::Value::Array(results)
            };

            let doc = Arc::new(DocumentBuilder::from_serde_value(
                value, None, 0, Duration::ZERO,
            ));
            let mut view = TreeView::new(doc);
            view.set_viewport_height(viewport_height);
            filter.result = Some(FilterResult { view });
        }
        Err(e) => {
            filter.error = Some(e.to_string());
            filter.result = None;
            filter.count = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render the filter input in a single status-bar row (replaces the status bar when active).
pub(crate) fn render_filter_input(
    frame: &mut ratatui::Frame,
    filter: &FilterState,
    area: Rect,
    theme: &Theme,
) {
    let prompt = " \u{276f} ";
    let prompt_width = crate::util::display_width(prompt) as u16;
    let (before, after) = filter.query.split_at(filter.cursor);

    // Right-aligned status
    let right = if let Some(ref err) = filter.error {
        format!("\u{26a0} {}", crate::util::truncate_chars(err, 30))
    } else if filter.count > 0 {
        format!("{} result{}", filter.count, if filter.count == 1 { "" } else { "s" })
    } else if filter.query.trim().is_empty() {
        String::new()
    } else {
        "no results".into()
    };

    let right_style = if filter.error.is_some() { theme.error_style } else { theme.fg_dim_style };
    let right_width = crate::util::display_width(&right);
    let left_avail = (area.width as usize).saturating_sub(right_width + prompt_width as usize + 2);
    let padding = left_avail.saturating_sub(
        crate::util::display_width(before) + crate::util::display_width(after),
    );

    let mut spans = vec![Span::styled(prompt, theme.toolbar_brand_style)];

    if filter.query.is_empty() {
        spans.push(Span::styled(". | keys", theme.tree_guide_style));
    } else {
        spans.push(Span::styled(before, theme.fg_style));
        spans.push(Span::styled(after, theme.fg_style));
    }

    spans.push(Span::raw(" ".repeat(padding)));
    spans.push(Span::styled(right, right_style));
    spans.push(Span::raw(" "));

    // Distinct background for filter mode
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(spans)).style(theme.toolbar_bg_style),
        area,
    );

    // Terminal cursor
    let cursor_x = area.x + prompt_width + if filter.query.is_empty() { 0 } else { crate::util::display_width(before) as u16 };
    frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, area.y));
}

/// Render the suggestion popup ABOVE the status bar.
pub(crate) fn render_filter_suggestions(
    frame: &mut ratatui::Frame,
    filter: &FilterState,
    status_area: Rect,
    theme: &Theme,
) {
    if !filter.active {
        return;
    }
    render_suggestions(frame, filter, status_area, theme);
}

fn render_suggestions(
    frame: &mut ratatui::Frame,
    filter: &FilterState,
    input_area: Rect,
    theme: &Theme,
) {
    if !filter.show_suggestions || filter.suggestions.is_empty() {
        return;
    }

    let max_shown = filter.suggestions.len().min(8);
    let popup_height = max_shown as u16 + 2;
    let popup_width = input_area.width.min(45);

    // Above the status bar
    let screen = frame.area();
    let popup_y = input_area.y.saturating_sub(popup_height);
    let popup_x = (input_area.x + 3).min(screen.width.saturating_sub(popup_width));
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    frame.render_widget(ratatui::widgets::Clear, popup_area);

    let block = ratatui::widgets::Block::bordered()
        .border_style(theme.tree_guide_style)
        .style(theme.bg_style);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let lines: Vec<Line> = filter
        .suggestions
        .iter()
        .take(max_shown)
        .enumerate()
        .map(|(i, s)| {
            let is_sel = i == filter.suggestion_idx;
            let bg = if is_sel { theme.selection_style } else { theme.bg_style };
            let is_builtin = BUILTINS.contains(&s.as_str());
            let icon = if is_builtin { "\u{0192}" } else { "\u{25cb}" }; // ƒ for builtins, ○ for fields
            Line::from(vec![
                Span::styled(format!(" {icon} "), if is_builtin { theme.boolean } else { theme.key }),
                Span::styled(format!("{s} "), bg),
            ]).style(bg)
        })
        .collect();

    frame.render_widget(ratatui::widgets::Paragraph::new(lines), inner);
}

// ---------------------------------------------------------------------------
// Suggestions engine
// ---------------------------------------------------------------------------

const BUILTINS: &[&str] = &[
    "length", "keys", "values", "type", "flatten",
    "first", "last", "reverse", "unique", "sort",
    "min", "max", "not", "to_number", "to_string",
    "ascii_downcase", "select", "map", "sort_by",
];

pub(crate) fn update_suggestions(
    filter: &mut FilterState,
    doc: &JsonDocument,
    root: crate::model::node::NodeId,
    cached_fields: &mut Option<Vec<String>>,
) {
    // Use text up to cursor for context detection (not the full query)
    let query_to_cursor = &filter.query[..filter.cursor];
    if query_to_cursor.is_empty() {
        filter.suggestions = vec![".".into()];
        return;
    }

    let ctx = detect_context(query_to_cursor);
    let prefix = completion_prefix(query_to_cursor);

    filter.suggestions = match ctx {
        Context::AfterDot => {
            let fields = cached_fields
                .get_or_insert_with(|| collect_field_names(doc, root));
            let mut out = fields.clone();
            if !prefix.is_empty() {
                let lower = prefix.to_lowercase();
                out.retain(|f| f.to_lowercase().starts_with(&lower));
            }
            out.truncate(20);
            out
        }
        Context::AfterPipe | Context::General => {
            let mut out: Vec<String> = BUILTINS.iter().map(|s| s.to_string()).collect();
            out.push(".".into());
            if !prefix.is_empty() {
                let lower = prefix.to_lowercase();
                out.retain(|s| s.to_lowercase().starts_with(&lower));
            }
            out.truncate(15);
            out
        }
    };

    // Clamp suggestion index
    if filter.suggestion_idx >= filter.suggestions.len() {
        filter.suggestion_idx = 0;
    }
}

enum Context { AfterDot, AfterPipe, General }

fn detect_context(text: &str) -> Context {
    let trimmed = text.trim_end();
    if trimmed.ends_with('.') {
        return Context::AfterDot;
    }
    if trimmed.ends_with('|') {
        return Context::AfterPipe;
    }
    match trimmed.rfind(['.', '|', '(', ' ']) {
        Some(i) => match trimmed.as_bytes()[i] {
            b'.' => Context::AfterDot,
            b'|' | b' ' => Context::AfterPipe,
            _ => Context::General,
        },
        None => Context::General,
    }
}

fn completion_prefix(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut i = bytes.len();
    while i > 0 && !matches!(bytes[i - 1], b'.' | b'|' | b'(' | b' ' | b')') {
        i -= 1;
    }
    &text[i..]
}

/// Rewrite a "simple" query into jq syntax if it doesn't already look like jq.
///
/// Examples:
///   "contractName"           → ".[] | .contractName"
///   "age > 30"               → ".[] | select(.age > 30)"
///   "name = Alice"           → ".[] | select(.name == \"Alice\")"
///   "contractYear != 2023"   → ".[] | select(.contractYear != 2023)"
fn collect_field_names(doc: &JsonDocument, root: crate::model::node::NodeId) -> Vec<String> {
    use crate::model::node::JsonValue;
    let mut fields = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![(root, 0u8)];

    while let Some((id, depth)) = stack.pop() {
        if depth > 2 { continue; }
        let node = doc.node(id);
        match &node.value {
            JsonValue::Object(entries) => {
                for (key, child_id) in entries {
                    if seen.insert(key.to_string()) {
                        fields.push(key.to_string());
                    }
                    stack.push((*child_id, depth + 1));
                }
            }
            JsonValue::Array(children) => {
                for &child_id in children.iter().take(10) {
                    stack.push((child_id, depth + 1));
                }
            }
            _ => {}
        }
    }

    fields.sort();
    fields
}
