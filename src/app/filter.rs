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
    pub(crate) error: Option<String>,
    pub(crate) showing_result: bool,
    pub(crate) result_view: Option<TreeView>,
    pub(crate) result_doc: Option<Arc<JsonDocument>>,
    pub(crate) suggestions: Vec<String>,
    pub(crate) suggestion_idx: usize,
    pub(crate) show_suggestions: bool,
    /// Live preview of filter results (updated as user types, debounced).
    pub(crate) live_results: Vec<String>,
    pub(crate) live_count: usize,
    pub(crate) live_error: Option<String>,
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
        }
    }

    pub(crate) fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.error = None;
        self.suggestions.clear();
        self.show_suggestions = false;
        self.live_results.clear();
        self.live_count = 0;
        self.live_error = None;
    }

    /// Reopen the filter input preserving the existing query.
    pub(crate) fn reopen(&mut self) {
        self.active = true;
        self.error = None;
        self.show_suggestions = false;
        self.live_results.clear();
        self.live_count = 0;
        self.live_error = None;
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
                    FilterAction::RunFilter
                }
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if !self.suggestions.is_empty() {
                    if self.show_suggestions {
                        self.suggestion_idx = (self.suggestion_idx + 1) % self.suggestions.len();
                    } else {
                        self.show_suggestions = true;
                        self.suggestion_idx = 0;
                    }
                }
                // No-op if suggestions empty — don't enter suggestion mode
                FilterAction::None
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) if self.show_suggestions => {
                // Reverse cycle
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
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.query.pop();
                self.error = None;
                self.show_suggestions = false;
                FilterAction::None
            }
            (mods, KeyCode::Char(c))
                if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if self.query.len() < 1024 {
                    self.query.push(c);
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
        // Find the partial token being typed to replace it
        let prefix = completion_prefix(&self.query);
        self.query.truncate(self.query.len() - prefix.len());
        self.query.push_str(&suggestion);
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
) {
    let screen = frame.area();
    let w = (screen.width * 80 / 100).max(50).min(screen.width);
    let h = (screen.height * 70 / 100).max(12).min(screen.height);
    let x = (screen.width - w) / 2;
    let y = (screen.height - h) / 2;
    let overlay = Rect::new(x, y, w, h);

    frame.render_widget(ratatui::widgets::Clear, overlay);

    let block = ratatui::widgets::Block::bordered()
        .title(" Filter ")
        .title_style(theme.help_title_style)
        .border_style(theme.tree_guide_style)
        .style(theme.bg_style);
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.height < 4 {
        return;
    }

    // Input line
    let input_area = Rect::new(inner.x, inner.y, inner.width, 1);
    let mut input_spans = vec![
        Span::styled(" \u{276f} ", theme.toolbar_brand_style),
        Span::styled(filter.query.as_str(), theme.fg_style),
        Span::styled("\u{2588}", theme.input_cursor_style),
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

    // Separator
    let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
    let result_label = if let Some(ref err) = filter.live_error {
        format!(" \u{26a0} {err}")
    } else if filter.live_count > 0 {
        format!(" {count} result{s}", count = filter.live_count, s = if filter.live_count == 1 { "" } else { "s" })
    } else if filter.query.trim().is_empty() {
        " Type an expression".into()
    } else {
        " No results".into()
    };

    let sep_style = if filter.live_error.is_some() {
        theme.error_style
    } else {
        theme.fg_dim_style
    };

    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(Span::styled(result_label, sep_style)))
            .style(theme.bg_style),
        sep_area,
    );

    // Results area
    let results_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(3));

    let mut lines: Vec<Line> = filter
        .live_results
        .iter()
        .take(results_area.height as usize)
        .map(|s| Line::from(Span::styled(format!("  {s}"), theme.fg_style)))
        .collect();

    if lines.is_empty() && filter.live_error.is_none() && !filter.query.trim().is_empty() {
        lines.push(Line::from(Span::styled("  (empty)", theme.fg_dim_style)));
    }

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

const BUILTINS: &[&str] = &[
    "length", "keys", "values", "type", "flatten",
    "first", "last", "reverse", "unique", "sort",
    "min", "max", "not", "to_number", "to_string",
    "ascii_downcase", "select", "map", "sort_by",
];

/// Compute live preview of filter results (called on each keystroke, debounced in caller).
pub(crate) fn update_live_preview(
    filter: &mut FilterState,
    document: &JsonDocument,
) {
    let query = filter.query.trim();
    if query.is_empty() {
        filter.live_results.clear();
        filter.live_count = 0;
        filter.live_error = None;
        return;
    }

    let expr = match crate::filter::parse::parse(query) {
        Ok(e) => e,
        Err(e) => {
            filter.live_error = Some(e.to_string());
            filter.live_results.clear();
            filter.live_count = 0;
            return;
        }
    };

    let root_value = raw::rebuild_serde_value(document, document.root());
    match crate::filter::eval::apply(&root_value, &expr) {
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
            // Suggest field names from document
            let mut fields = collect_field_names(doc, root);
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
