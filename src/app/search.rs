use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::search::SearchHit;
use crate::theme::Theme;

/// Debounce delay before triggering search after the last keystroke.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

pub(crate) struct SearchState {
    pub(crate) active: bool,
    pub(crate) query: String,
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) current_hit: usize,
    pub(crate) dirty: bool,
    last_keystroke: Instant,
    pub(crate) regex_mode: bool,
}

pub(crate) enum SearchAction {
    None,
    Close,
    RunSearchAndClose,
    Navigate,
    QueryChanged,
    ToggleRegex,
}

impl SearchState {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            hits: Vec::new(),
            current_hit: 0,
            dirty: false,
            last_keystroke: Instant::now(),
            regex_mode: false,
        }
    }

    pub(crate) fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.hits.clear();
        self.current_hit = 0;
        self.dirty = false;
    }

    pub(crate) fn close(&mut self) {
        self.active = false;
        self.dirty = false;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_keystroke = Instant::now();
    }

    pub(crate) fn should_search(&self) -> bool {
        self.dirty && self.last_keystroke.elapsed() >= SEARCH_DEBOUNCE
    }

    pub(crate) fn next_hit(&mut self) {
        if !self.hits.is_empty() {
            self.current_hit = (self.current_hit + 1) % self.hits.len();
        }
    }

    pub(crate) fn prev_hit(&mut self) {
        if !self.hits.is_empty() {
            self.current_hit = if self.current_hit == 0 {
                self.hits.len() - 1
            } else {
                self.current_hit - 1
            };
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> SearchAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => SearchAction::Close,
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if self.dirty {
                    SearchAction::RunSearchAndClose
                } else {
                    SearchAction::Close
                }
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.query.pop();
                self.mark_dirty();
                SearchAction::QueryChanged
            }
            (mods, KeyCode::Char(c))
                if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.query.push(c);
                self.mark_dirty();
                SearchAction::QueryChanged
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Down) => {
                self.next_hit();
                SearchAction::Navigate
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) | (KeyModifiers::NONE, KeyCode::Up) => {
                self.prev_hit();
                SearchAction::Navigate
            }
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.regex_mode = !self.regex_mode;
                self.mark_dirty();
                SearchAction::ToggleRegex
            }
            _ => SearchAction::None,
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

pub(crate) struct SearchBar<'a> {
    pub(crate) state: &'a SearchState,
    pub(crate) theme: &'a Theme,
}

impl Widget for SearchBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let search = self.state;
        let theme = self.theme;

        let no_matches = search.hits.is_empty() && !search.query.is_empty();
        let hit_info = if search.hits.is_empty() {
            if search.query.is_empty() {
                String::new()
            } else {
                " No matches".to_string()
            }
        } else {
            format!(" {}/{}", search.current_hit + 1, search.hits.len())
        };

        let mut spans = vec![Span::styled(" / ", theme.toolbar_brand_style)];

        if search.regex_mode {
            spans.push(Span::styled(
                "[.*] ",
                theme.input_cursor_style.add_modifier(Modifier::BOLD),
            ));
        }

        let hit_info_style = if no_matches {
            theme.error_style
        } else {
            theme.fg_dim_style
        };
        spans.extend([
            Span::styled(search.query.as_str(), theme.fg_style),
            Span::styled("\u{2588}", theme.input_cursor_style),
            Span::styled(hit_info, hit_info_style),
        ]);

        let line = Line::from(spans);
        ratatui::widgets::Paragraph::new(line)
            .style(theme.bg_style)
            .render(area, buf);
    }
}
