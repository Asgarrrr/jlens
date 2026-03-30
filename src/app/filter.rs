use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::model::node::{DocumentBuilder, JsonDocument};
use crate::theme::Theme;
use crate::views::View;
use crate::views::raw;
use crate::views::tree::TreeView;

pub(crate) struct FilterState {
    pub(crate) active: bool,
    pub(crate) query: String,
    pub(crate) cursor: usize, // byte offset in query
    pub(crate) error: Option<String>,
    pub(crate) showing_result: bool,
    pub(crate) result_view: Option<TreeView>,
    pub(crate) result_doc: Option<Arc<JsonDocument>>,
    pub(crate) suggestions: Vec<String>,
    pub(crate) suggestion_idx: usize,
    pub(crate) show_suggestions: bool,
    pub(crate) live_results: Vec<String>,
    pub(crate) live_count: usize,
    pub(crate) live_error: Option<String>,
    /// Filter expression history (persists across open/close).
    pub(crate) history: Vec<String>,
    history_idx: Option<usize>,
    history_draft: String,
}

pub(crate) enum FilterAction {
    None,
    CloseInput,
    RunFilter,
    CloseResult,
    ReopenInput,
    DelegateToResult(KeyEvent),
}

impl FilterState {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            cursor: 0,
            error: None,
            showing_result: false,
            result_view: None,
            result_doc: None,
            suggestions: Vec::new(),
            suggestion_idx: 0,
            show_suggestions: false,
            live_results: Vec::new(),
            live_count: 0,
            live_error: None,
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
        self.suggestions.clear();
        self.show_suggestions = false;
        self.live_results.clear();
        self.live_count = 0;
        self.live_error = None;
        self.history_idx = None;
        self.history_draft.clear();
    }

    /// Reopen preserving the existing query.
    pub(crate) fn reopen(&mut self) {
        self.active = true;
        self.cursor = self.query.len();
        self.error = None;
        self.show_suggestions = false;
        self.live_results.clear();
        self.live_count = 0;
        self.live_error = None;
        self.history_idx = None;
    }

    pub(crate) fn close_input(&mut self) {
        self.active = false;
        self.error = None;
    }

    pub(crate) fn close_result(&mut self) {
        self.showing_result = false;
        self.result_view = None;
        self.result_doc = None;
    }

    pub(crate) fn handle_input_key(&mut self, key: KeyEvent) -> FilterAction {
        match (key.modifiers, key.code) {
            // --- Modal controls ---
            (KeyModifiers::NONE, KeyCode::Esc) => {
                if self.show_suggestions {
                    self.show_suggestions = false;
                    FilterAction::None
                } else {
                    FilterAction::CloseInput
                }
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if self.show_suggestions {
                    self.accept_suggestion();
                    FilterAction::None
                } else {
                    // Save to history before running
                    let q = self.query.trim().to_string();
                    if !q.is_empty() {
                        self.history.retain(|h| h != &q);
                        self.history.push(q);
                    }
                    FilterAction::RunFilter
                }
            }

            // --- Suggestions ---
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if !self.suggestions.is_empty() {
                    if self.show_suggestions {
                        self.suggestion_idx = (self.suggestion_idx + 1) % self.suggestions.len();
                    } else {
                        self.show_suggestions = true;
                        self.suggestion_idx = 0;
                    }
                }
                FilterAction::None
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) if self.show_suggestions => {
                if !self.suggestions.is_empty() {
                    self.suggestion_idx = if self.suggestion_idx == 0 {
                        self.suggestions.len() - 1
                    } else {
                        self.suggestion_idx - 1
                    };
                }
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Down) if self.show_suggestions => {
                if self.suggestion_idx + 1 < self.suggestions.len() {
                    self.suggestion_idx += 1;
                }
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Up) if self.show_suggestions => {
                self.suggestion_idx = self.suggestion_idx.saturating_sub(1);
                FilterAction::None
            }

            // --- History ---
            (KeyModifiers::NONE, KeyCode::Up) if !self.show_suggestions => {
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
                    }
                }
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Down) if !self.show_suggestions => {
                if let Some(idx) = self.history_idx {
                    if idx + 1 < self.history.len() {
                        self.history_idx = Some(idx + 1);
                        self.query = self.history[idx + 1].clone();
                    } else {
                        self.history_idx = None;
                        self.query = self.history_draft.clone();
                    }
                    self.cursor = self.query.len();
                }
                FilterAction::None
            }

            // --- Cursor movement ---
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.cursor > 0 {
                    // Move back one char
                    let prev = self.query[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.cursor = prev;
                }
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                if self.cursor < self.query.len() {
                    let next = self.query[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor + i)
                        .unwrap_or(self.query.len());
                    self.cursor = next;
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

            // --- Editing ---
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if self.cursor > 0 {
                    let prev = self.query[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.query.drain(prev..self.cursor);
                    self.cursor = prev;
                }
                self.error = None;
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
                }
                self.error = None;
                FilterAction::None
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                // Clear line before cursor
                self.query.drain(..self.cursor);
                self.cursor = 0;
                self.error = None;
                self.show_suggestions = false;
                FilterAction::None
            }
            (mods, KeyCode::Char(c))
                if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if self.query.len() < 1024 {
                    self.query.insert(self.cursor, c);
                    self.cursor += c.len_utf8();
                    self.error = None;
                    self.show_suggestions = false;
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
        let prefix = completion_prefix(&self.query[..self.cursor]);
        let prefix_start = self.cursor - prefix.len();
        self.query.replace_range(prefix_start..self.cursor, &suggestion);
        self.cursor = prefix_start + suggestion.len();
        self.show_suggestions = false;
        self.suggestion_idx = 0;
    }

    pub(crate) fn handle_result_key(&mut self, key: KeyEvent) -> FilterAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => FilterAction::CloseResult,
            (KeyModifiers::NONE, KeyCode::Char(':')) => FilterAction::ReopenInput,
            _ => FilterAction::DelegateToResult(key),
        }
    }
}

/// Parse and apply the current filter query, updating `filter` state.
pub(crate) fn run_filter(
    filter: &mut FilterState,
    document: &JsonDocument,
    viewport_height: usize,
) {
    let query = filter.query.trim();

    // Parse
    let expr = match crate::filter::parse::parse(query) {
        Ok(e) => e,
        Err(e) => {
            filter.error = Some(e.to_string());
            return;
        }
    };

    // Rebuild the serde value from the document root
    let root_value = raw::rebuild_serde_value(document, document.root());

    // Evaluate
    let results = match crate::filter::eval::apply(&root_value, &expr) {
        Ok(r) => r,
        Err(e) => {
            filter.error = Some(e.to_string());
            return;
        }
    };

    if results.is_empty() {
        filter.error = Some("No matching results".to_string());
        return;
    }

    // Wrap multiple results in an array; a single result stands on its own.
    let result_value = if results.len() == 1 {
        results.into_iter().next().unwrap()
    } else {
        serde_json::Value::Array(results)
    };

    // Build a JsonDocument from the result value.
    let result_doc = Arc::new(DocumentBuilder::from_serde_value(
        result_value,
        None,
        0,
        Duration::ZERO,
    ));

    let mut result_view = TreeView::new(Arc::clone(&result_doc));
    result_view.set_viewport_height(viewport_height);

    filter.result_doc = Some(result_doc);
    filter.result_view = Some(result_view);
    filter.error = None;
    filter.showing_result = true;
    filter.close_input();
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Render the filter as a centered overlay with live results.
pub(crate) fn render_filter_overlay(
    frame: &mut ratatui::Frame,
    filter: &FilterState,
    theme: &Theme,
    field_names: &[String],
) {
    let screen = frame.area();
    // Size to content: input + separator + results (max 12) + footer = ~16 rows
    let content_rows = if filter.query.trim().is_empty() { 12 } else { filter.live_results.len().clamp(1, 15) + 1 };
    let h = (content_rows as u16 + 4).clamp(8, screen.height.saturating_sub(4));
    let w = (screen.width * 70 / 100).clamp(50, screen.width.saturating_sub(4));
    let x = (screen.width - w) / 2;
    let y = (screen.height - h) / 2;
    let overlay = Rect::new(x, y, w, h);

    frame.render_widget(ratatui::widgets::Clear, overlay);

    let block = ratatui::widgets::Block::bordered()
        .title(" Filter ")
        .title_style(theme.toolbar_brand_style)
        .border_style(theme.toolbar_active_style)
        .style(theme.bg_style);
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.height < 4 {
        return;
    }

    // Input line with cursor at correct position
    let input_area = Rect::new(inner.x, inner.y, inner.width, 1);
    let (before_cursor, after_cursor) = filter.query.split_at(filter.cursor);
    let mut input_spans = vec![
        Span::styled(" \u{276f} ", theme.toolbar_brand_style),
        Span::styled(before_cursor, theme.fg_style),
        Span::styled("\u{2588}", theme.input_cursor_style),
        Span::styled(after_cursor, theme.fg_style),
    ];

    if filter.show_suggestions {
        input_spans.push(Span::styled(
            "  [Tab] accept  [\u{2191}\u{2193}] nav",
            theme.fg_dim_style,
        ));
    }

    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(input_spans)).style(theme.bg_style),
        input_area,
    );

    // Separator with label
    let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
    let (sep_label, sep_style) = if let Some(ref err) = filter.live_error {
        (format!(" \u{26a0} {err} "), theme.error_style)
    } else if filter.live_count > 0 {
        let s = if filter.live_count == 1 { "" } else { "s" };
        (format!(" {} result{s} ", filter.live_count), theme.fg_dim_style)
    } else if filter.query.trim().is_empty() {
        (" Examples ".into(), theme.fg_dim_style)
    } else {
        (" No results ".into(), theme.fg_dim_style)
    };

    // Draw: ─── label ──────
    let label_width = sep_label.len();
    let remaining = (inner.width as usize).saturating_sub(label_width + 2);
    let rule_right = "\u{2500}".repeat(remaining);
    let sep_line = Line::from(vec![
        Span::styled("\u{2500}\u{2500}", theme.tree_guide_style),
        Span::styled(sep_label, sep_style),
        Span::styled(rule_right, theme.tree_guide_style),
    ]);

    frame.render_widget(
        ratatui::widgets::Paragraph::new(sep_line).style(theme.bg_style),
        sep_area,
    );

    // Results area
    let results_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(3));

    let lines: Vec<Line> = if filter.query.trim().is_empty() {
        // Show contextual examples when empty
        render_examples(field_names, theme, results_area.height as usize)
    } else if !filter.live_results.is_empty() {
        filter
            .live_results
            .iter()
            .take(results_area.height as usize)
            .map(|s| Line::from(Span::styled(format!("  {s}"), theme.fg_style)))
            .collect()
    } else if filter.live_error.is_some() {
        vec![]
    } else {
        vec![Line::from(Span::styled("  (empty)", theme.fg_dim_style))]
    };

    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines).style(theme.bg_style),
        results_area,
    );

    // Footer hints
    let footer_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            " [Enter] apply  [Tab] suggest  [Esc] cancel",
            theme.fg_dim_style,
        )))
        .style(theme.bg_style),
        footer_area,
    );

    // Autocomplete popup on top
    render_suggestions(frame, filter, input_area, theme);
}

// ---------------------------------------------------------------------------
// Suggestion popup (rendered separately, above the filter bar)
// ---------------------------------------------------------------------------

pub(crate) fn render_suggestions(
    frame: &mut ratatui::Frame,
    filter: &FilterState,
    bar_area: Rect,
    theme: &Theme,
) {
    if !filter.show_suggestions || filter.suggestions.is_empty() {
        return;
    }

    let max_shown = filter.suggestions.len().min(8);
    let popup_height = max_shown as u16 + 2; // +2 for top/bottom border
    let popup_width = bar_area.width.min(45);

    // Position above the input line, clamped to stay inside the overlay
    let screen = frame.area();
    let popup_y = bar_area.y.saturating_sub(popup_height).max(1); // never overlap toolbar
    let popup_x = (bar_area.x + 3).min(screen.width.saturating_sub(popup_width));
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear + bordered block (ratatui popup pattern)
    frame.render_widget(ratatui::widgets::Clear, popup_area);

    let block = ratatui::widgets::Block::bordered()
        .border_style(theme.tree_guide_style)
        .style(theme.bg_style);

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Render suggestions inside the block
    let mut lines = Vec::new();
    for (i, suggestion) in filter.suggestions.iter().take(max_shown).enumerate() {
        let is_selected = i == filter.suggestion_idx;
        let style = if is_selected {
            theme.selection_style
        } else {
            theme.bg_style
        };
        lines.push(Line::from(Span::styled(format!(" {suggestion} "), style)));
    }

    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Suggestion engine
// ---------------------------------------------------------------------------

fn render_examples(fields: &[String], theme: &Theme, max_lines: usize) -> Vec<Line<'static>> {
    let sample = fields
        .iter()
        .find(|f| f.as_str() != ".")
        .map(|s| s.as_str())
        .unwrap_or("name");

    let examples: Vec<(&str, String)> = vec![
        ("Count", ". | length".into()),
        ("Values", format!(".[] | .{sample}")),
        ("Keys", ".[0] | keys".into()),
        ("Filter", format!(".[] | select(.{sample} != null)")),
        ("Sort", format!("sort_by(.{sample})")),
        ("Extract", format!("map(.{sample})")),
        ("Unique", format!("map(.{sample}) | unique")),
        ("Slice", ".[0:5]".into()),
    ];

    examples
        .into_iter()
        .take(max_lines)
        .map(|(desc, expr)| {
            Line::from(vec![
                Span::styled(format!("  {desc:<10}"), theme.fg_dim_style),
                Span::styled(expr, theme.key),
            ])
        })
        .collect()
}

const BUILTINS: &[&str] = &[
    "length", "keys", "values", "type", "flatten",
    "first", "last", "reverse", "unique", "sort",
    "min", "max", "not", "to_number", "to_string",
    "ascii_downcase", "select", "map", "sort_by",
];

/// Compute live preview of filter results.
/// Caches the serde_json::Value reconstruction to avoid rebuilding on every keystroke.
pub(crate) fn update_live_preview(
    filter: &mut FilterState,
    document: &JsonDocument,
    cached_value: &mut Option<serde_json::Value>,
) {
    let query = filter.query.trim();
    if query.is_empty() {
        filter.live_results.clear();
        filter.live_count = 0;
        filter.live_error = None;
        return;
    }

    // Parse first — cheap. If it fails, show error without touching the cache.
    let expr = match crate::filter::parse::parse(query) {
        Ok(e) => e,
        Err(e) => {
            filter.live_error = Some(e.to_string());
            filter.live_results.clear();
            filter.live_count = 0;
            return;
        }
    };

    // Rebuild serde_json::Value only once (cached across keystrokes).
    let root_value = cached_value
        .get_or_insert_with(|| raw::rebuild_serde_value(document, document.root()));

    match crate::filter::eval::apply(root_value, &expr) {
        Ok(results) => {
            filter.live_count = results.len();
            filter.live_error = None;
            filter.live_results = results
                .iter()
                .take(20)
                .map(|v| {
                    let s = serde_json::to_string(v).unwrap_or_default();
                    if s.len() > 120 {
                        format!("{}...", crate::util::truncate_chars(&s, 117))
                    } else {
                        s
                    }
                })
                .collect();
        }
        Err(e) => {
            filter.live_error = Some(e.to_string());
            filter.live_results.clear();
            filter.live_count = 0;
        }
    }
}

/// Update suggestions based on the current query context and document.
pub(crate) fn update_suggestions(
    filter: &mut FilterState,
    doc: &JsonDocument,
    root: crate::model::node::NodeId,
    cached_fields: &mut Option<Vec<String>>,
) {
    let query = &filter.query;
    if query.is_empty() {
        filter.suggestions = vec![".".into()];
        return;
    }

    let ctx = detect_context(query);
    let prefix = completion_prefix(query);

    filter.suggestions = match ctx {
        Context::AfterDot => {
            let all_fields = cached_fields
                .get_or_insert_with(|| collect_field_names(doc, root));
            let mut fields = all_fields.clone();
            if !prefix.is_empty() {
                let lower = prefix.to_lowercase();
                fields.retain(|f| f.to_lowercase().starts_with(&lower));
            }
            fields.truncate(20);
            fields
        }
        Context::AfterPipe => {
            // Suggest builtins
            let mut builtins: Vec<String> = BUILTINS.iter().map(|s| s.to_string()).collect();
            builtins.push(".".into());
            if !prefix.is_empty() {
                let lower = prefix.to_lowercase();
                builtins.retain(|b| b.to_lowercase().starts_with(&lower));
            }
            builtins
        }
        Context::General => {
            let mut all: Vec<String> = BUILTINS.iter().map(|s| s.to_string()).collect();
            all.push(".".into());
            if !prefix.is_empty() {
                let lower = prefix.to_lowercase();
                all.retain(|s| s.to_lowercase().starts_with(&lower));
            }
            all.truncate(15);
            all
        }
    };
}

#[derive(Debug)]
enum Context {
    AfterDot,
    AfterPipe,
    General,
}

fn detect_context(query: &str) -> Context {
    let trimmed = query.trim_end();
    if trimmed.ends_with('.') {
        Context::AfterDot
    } else if trimmed.ends_with('|') {
        Context::AfterPipe
    } else {
        // Check what the last "word separator" was
        let last_sep = trimmed.rfind(['.', '|', '(', ' ']);
        match last_sep {
            Some(i) => {
                let sep = trimmed.as_bytes()[i];
                match sep {
                    b'.' => Context::AfterDot,
                    b'|' | b' ' => Context::AfterPipe,
                    b'(' => Context::General,
                    _ => Context::General,
                }
            }
            None => Context::General,
        }
    }
}

fn completion_prefix(query: &str) -> &str {
    let bytes = query.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        let b = bytes[i - 1];
        if b == b'.' || b == b'|' || b == b'(' || b == b' ' || b == b')' {
            break;
        }
        i -= 1;
    }
    &query[i..]
}

fn collect_field_names(doc: &JsonDocument, root: crate::model::node::NodeId) -> Vec<String> {
    use crate::model::node::JsonValue;
    let mut fields = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Walk the first 2 levels from root to collect field names
    let mut stack = vec![(root, 0u8)];
    while let Some((id, depth)) = stack.pop() {
        if depth > 2 {
            continue;
        }
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
