use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::model::node::{DocumentBuilder, JsonDocument};
use crate::theme::Theme;
use crate::views::raw;
use crate::views::tree::TreeView;
use crate::views::View;

pub(crate) struct FilterState {
    pub(crate) active: bool,
    pub(crate) query: String,
    pub(crate) error: Option<String>,
    pub(crate) showing_result: bool,
    pub(crate) result_view: Option<TreeView>,
    pub(crate) result_doc: Option<Arc<JsonDocument>>,
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
        }
    }

    pub(crate) fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.error = None;
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
            (KeyModifiers::NONE, KeyCode::Esc) => FilterAction::CloseInput,
            (KeyModifiers::NONE, KeyCode::Enter) => FilterAction::RunFilter,
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.query.pop();
                self.error = None;
                FilterAction::None
            }
            (KeyModifiers::NONE, KeyCode::Char(c)) => {
                self.query.push(c);
                self.error = None;
                FilterAction::None
            }
            _ => FilterAction::None,
        }
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
    let query = filter.query.trim().to_string();

    // Parse
    let expr = match crate::filter::parse::parse(&query) {
        Ok(e) => e,
        Err(e) => {
            filter.error = Some(e.to_string());
            return;
        }
    };

    // Rebuild the serde value from the document root
    let root_value = raw::rebuild_serde_value(document, document.root());

    // Evaluate
    let results = match crate::filter::eval::eval(&root_value, &expr) {
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

pub(crate) struct FilterBar<'a> {
    pub(crate) state: &'a FilterState,
    pub(crate) theme: &'a Theme,
}

impl Widget for FilterBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let filter = self.state;
        let theme = self.theme;

        let mut spans = vec![Span::styled(
            " : ",
            Style::new()
                .fg(theme.toolbar_active_fg)
                .bg(theme.toolbar_active_bg)
                .add_modifier(Modifier::BOLD),
        )];

        spans.push(Span::styled(
            filter.query.clone(),
            Style::new().fg(theme.fg).bg(theme.bg),
        ));
        spans.push(Span::styled(
            "\u{2588}",
            Style::new().fg(theme.toolbar_active_bg).bg(theme.bg),
        ));

        if let Some(ref err) = filter.error {
            spans.push(Span::styled(
                format!("  \u{26a0} {}", err),
                Style::new()
                    .fg(theme.string.fg.unwrap_or(theme.fg_dim))
                    .bg(theme.bg),
            ));
        } else {
            spans.push(Span::styled(
                "  [Enter] run  [Esc] cancel",
                Style::new().fg(theme.fg_dim).bg(theme.bg),
            ));
        }

        let line = Line::from(spans);
        ratatui::widgets::Paragraph::new(line)
            .style(Style::new().bg(theme.bg))
            .render(area, buf);
    }
}
