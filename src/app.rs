mod diff;
mod terminal;

use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::event::{AppEvent, EventReader};
use crate::input;
use crate::model::lazy::LazyDocument;
use crate::model::node::{JsonDocument, NodeId};
use crate::parser;
use crate::search::{self, SearchHit, SearchOptions};
use crate::theme::Theme;
use crate::ui::{self, UiLayout};
use crate::views::path::PathView;
use crate::views::raw::{self, RawView};
use crate::views::stats::StatsView;
use crate::views::table::TableView;
use crate::views::tree::TreeView;
use crate::views::{View, ViewAction, ViewMode};

// ---------------------------------------------------------------------------
// Search state
// ---------------------------------------------------------------------------

/// Debounce delay before triggering search after the last keystroke.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

struct SearchState {
    active: bool,
    query: String,
    hits: Vec<SearchHit>,
    current_hit: usize,
    /// Set when the query changed; cleared after the search actually runs.
    dirty: bool,
    last_keystroke: Instant,
    /// When true the query is interpreted as a regular expression.
    regex_mode: bool,
}

impl SearchState {
    fn new() -> Self {
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

    fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.hits.clear();
        self.current_hit = 0;
        self.dirty = false;
    }

    fn close(&mut self) {
        self.active = false;
        self.dirty = false;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_keystroke = Instant::now();
    }

    fn should_search(&self) -> bool {
        self.dirty && self.last_keystroke.elapsed() >= SEARCH_DEBOUNCE
    }

    fn next_hit(&mut self) {
        if !self.hits.is_empty() {
            self.current_hit = (self.current_hit + 1) % self.hits.len();
        }
    }

    fn prev_hit(&mut self) {
        if !self.hits.is_empty() {
            self.current_hit = if self.current_hit == 0 {
                self.hits.len() - 1
            } else {
                self.current_hit - 1
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Export state
// ---------------------------------------------------------------------------

struct ExportState {
    active: bool,
    filename: String,
}

impl ExportState {
    fn new() -> Self {
        Self {
            active: false,
            filename: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Filter state
// ---------------------------------------------------------------------------

struct FilterState {
    /// True while the filter input bar is open for typing.
    active: bool,
    /// The expression typed by the user.
    query: String,
    /// Parse or eval error message, shown in the bar.
    error: Option<String>,
    /// True while the result tree is displayed.
    showing_result: bool,
    /// The result tree view (if a successful eval has been run).
    result_view: Option<TreeView>,
    /// Backing document for the result tree.
    result_doc: Option<Arc<JsonDocument>>,
}

impl FilterState {
    fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            error: None,
            showing_result: false,
            result_view: None,
            result_doc: None,
        }
    }

    fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.error = None;
    }

    fn close_input(&mut self) {
        self.active = false;
        self.error = None;
    }

    fn close_result(&mut self) {
        self.showing_result = false;
        self.result_view = None;
        self.result_doc = None;
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct App {
    document: Arc<JsonDocument>,
    theme: Theme,
    active_mode: ViewMode,
    tree_view: TreeView,
    /// Non-tree views are constructed lazily on first access to reduce startup time.
    raw_view: Option<RawView>,
    table_view: Option<TableView>,
    path_view: Option<PathView>,
    stats_view: Option<StatsView>,
    last_viewport_height: usize,
    search: SearchState,
    clipboard: Option<arboard::Clipboard>,
    /// Transient status message (e.g. "Copied!"), cleared after a few ticks.
    flash_message: Option<(String, u8)>,
    show_help: bool,
    should_quit: bool,
    /// Last known main content area; updated each draw, used for mouse hit-testing.
    last_main_area: Rect,
    /// Last known status bar area; used for breadcrumb click navigation.
    last_status_area: Rect,
    export: ExportState,
    filter: FilterState,
    /// When set, the document was loaded lazily and stubs can be expanded.
    lazy_doc: Option<LazyDocument>,
}

impl App {
    fn new(document: Arc<JsonDocument>, theme: Theme) -> Self {
        let tree_view = TreeView::new(Arc::clone(&document));

        Self {
            document,
            theme,
            active_mode: ViewMode::Tree,
            tree_view,
            raw_view: None,
            table_view: None,
            path_view: None,
            stats_view: None,
            last_viewport_height: 0,
            search: SearchState::new(),
            clipboard: arboard::Clipboard::new().ok(),
            flash_message: None,
            show_help: false,
            should_quit: false,
            last_main_area: Rect::default(),
            last_status_area: Rect::default(),
            export: ExportState::new(),
            filter: FilterState::new(),
            lazy_doc: None,
        }
    }

    /// Initialize the App with a lazy document, setting stub IDs on the tree view.
    fn set_lazy_document(&mut self, lazy: LazyDocument) {
        let stubs: HashSet<NodeId> = lazy.stub_ids().collect();
        self.tree_view.set_stub_ids(stubs);
        self.lazy_doc = Some(lazy);
    }

    /// Expand a lazy stub node, rebuilding the document and updating views.
    fn expand_lazy_stub(&mut self, stub_id: NodeId) {
        let lazy = match self.lazy_doc.take() {
            Some(l) => l,
            None => return,
        };

        match lazy.expand_node(stub_id) {
            Ok(expanded) => {
                let doc = Arc::new(expanded.to_document());
                self.document = Arc::clone(&doc);
                self.tree_view.update_document(doc, Some(stub_id));

                // Invalidate lazily-constructed views so they rebuild with new data.
                self.raw_view = None;
                self.table_view = None;
                self.path_view = None;
                self.stats_view = None;

                // Update stub IDs and store the new lazy doc.
                let stubs: HashSet<NodeId> = expanded.stub_ids().collect();
                self.tree_view.set_stub_ids(stubs);
                self.lazy_doc = Some(expanded);
            }
            Err(e) => {
                self.flash_message = Some((format!("Expand failed: {}", e), 6));
                self.lazy_doc = Some(lazy);
            }
        }
    }

    /// Ensure the view for `mode` exists, constructing it lazily if needed.
    fn ensure_view(&mut self, mode: ViewMode) {
        let h = self.last_viewport_height;
        match mode {
            ViewMode::Tree => {}
            ViewMode::Raw => {
                if self.raw_view.is_none() {
                    let mut v = RawView::new(&self.document);
                    v.set_viewport_height(h);
                    self.raw_view = Some(v);
                }
            }
            ViewMode::Table => {
                if self.table_view.is_none() {
                    let mut v = TableView::new(Arc::clone(&self.document));
                    v.set_viewport_height(h);
                    self.table_view = Some(v);
                }
            }
            ViewMode::Paths => {
                if self.path_view.is_none() {
                    let mut v = PathView::new(Arc::clone(&self.document));
                    v.set_viewport_height(h);
                    self.path_view = Some(v);
                }
            }
            ViewMode::Stats => {
                if self.stats_view.is_none() {
                    let mut v = StatsView::new(Arc::clone(&self.document), &self.theme);
                    v.set_viewport_height(h);
                    self.stats_view = Some(v);
                }
            }
        }
    }

    fn active_view(&self) -> &dyn View {
        match self.active_mode {
            ViewMode::Tree => &self.tree_view,
            ViewMode::Raw => self.raw_view.as_ref().expect("view not initialized"),
            ViewMode::Table => self.table_view.as_ref().expect("view not initialized"),
            ViewMode::Paths => self.path_view.as_ref().expect("view not initialized"),
            ViewMode::Stats => self.stats_view.as_ref().expect("view not initialized"),
        }
    }

    fn active_view_mut(&mut self) -> &mut dyn View {
        match self.active_mode {
            ViewMode::Tree => &mut self.tree_view,
            ViewMode::Raw => self.raw_view.as_mut().expect("view not initialized"),
            ViewMode::Table => self.table_view.as_mut().expect("view not initialized"),
            ViewMode::Paths => self.path_view.as_mut().expect("view not initialized"),
            ViewMode::Stats => self.stats_view.as_mut().expect("view not initialized"),
        }
    }

    fn click_row(&mut self, row_in_viewport: usize) {
        self.active_view_mut().click_row(row_in_viewport);
    }

    fn update_viewport_height(&mut self, height: usize) {
        self.last_viewport_height = height;
        self.tree_view.set_viewport_height(height);
        if let Some(ref mut v) = self.raw_view { v.set_viewport_height(height); }
        if let Some(ref mut v) = self.table_view { v.set_viewport_height(height); }
        if let Some(ref mut v) = self.path_view { v.set_viewport_height(height); }
        if let Some(ref mut v) = self.stats_view { v.set_viewport_height(height); }
        if let Some(ref mut v) = self.filter.result_view { v.set_viewport_height(height); }
    }

    fn run_search(&mut self) {
        self.search.dirty = false;
        let mut opts = SearchOptions::default();
        opts.regex_mode = self.search.regex_mode;
        self.search.hits = search::search(&self.document, &self.search.query, &opts);
        self.search.current_hit = 0;

        // Feed matching node IDs to the tree view for O(1) highlight lookup
        let match_ids: HashSet<NodeId> = self.search.hits.iter().map(|h| h.node_id).collect();
        self.tree_view.set_search_matches(match_ids);

        self.navigate_to_current_hit();
    }

    fn navigate_to_current_hit(&mut self) {
        if let Some(hit) = self.search.hits.get(self.search.current_hit) {
            let node_id = hit.node_id;
            self.tree_view.set_current_search_node(Some(node_id));
            self.tree_view.navigate_to_node(node_id);
        } else {
            self.tree_view.set_current_search_node(None);
        }
    }
}

pub use diff::run_diff;

/// Run with a file path.
pub fn run_file(path: &Path, theme: Theme) -> Result<()> {
    match parser::parse_file_ex(path) {
        Ok(parser::ParseOutcome::Full(document)) => {
            run_with_document(Arc::new(document), None, theme)
        }
        Ok(parser::ParseOutcome::Lazy(lazy)) => {
            let document = Arc::new(lazy.to_document());
            run_with_document(document, Some(lazy), theme)
        }
        Err(crate::parser::ParseError::Syntax { line, column, message }) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: invalid JSON in \x1b[1m{}\x1b[0m",
                path.display()
            );
            eprintln!("  --> line {}, column {}", line, column);
            eprintln!("  {}", message);

            // Try to show the offending line from the file
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Some(error_line) = content.lines().nth(line.saturating_sub(1)) {
                    eprintln!();
                    eprintln!("  \x1b[2m{:>4} |\x1b[0m {}", line, error_line);
                    if column > 0 {
                        eprintln!(
                            "  \x1b[2m     |\x1b[0m \x1b[1;31m{}^\x1b[0m",
                            " ".repeat(column.saturating_sub(1))
                        );
                    }
                }
            }
            std::process::exit(1);
        }
        Err(e) => Err(e).with_context(|| format!("Failed to open {}", path.display())),
    }
}

/// Run reading JSON from stdin.
pub fn run_stdin(theme: Theme) -> Result<()> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("Failed to read from stdin")?;

    let start = std::time::Instant::now();
    let value: serde_json::Value = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(err) => {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: invalid JSON from stdin"
            );
            eprintln!("  --> line {}, column {}", err.line(), err.column());
            eprintln!("  {}", err);
            if let Some(error_line) = buf.lines().nth(err.line().saturating_sub(1)) {
                eprintln!();
                eprintln!("  \x1b[2m{:>4} |\x1b[0m {}", err.line(), error_line);
                if err.column() > 0 {
                    eprintln!(
                        "  \x1b[2m     |\x1b[0m \x1b[1;31m{}^\x1b[0m",
                        " ".repeat(err.column().saturating_sub(1))
                    );
                }
            }
            std::process::exit(1);
        }
    };
    let parse_time = start.elapsed();

    let source_size = buf.len() as u64;
    let document = crate::model::node::DocumentBuilder::from_serde_value(
        value,
        None,
        source_size,
        parse_time,
    );
    run_with_document(Arc::new(document), None, theme)
}

fn run_with_document(
    document: Arc<JsonDocument>,
    lazy: Option<LazyDocument>,
    theme: Theme,
) -> Result<()> {
    terminal::with_terminal(|t| run_app(t, document, lazy, theme))
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    document: Arc<JsonDocument>,
    lazy: Option<LazyDocument>,
    theme: Theme,
) -> Result<()> {
    let mut app = App::new(document, theme);
    if let Some(lazy) = lazy {
        app.set_lazy_document(lazy);
    }
    let events = EventReader::new(Duration::from_millis(100));

    loop {
        terminal.draw(|frame| {
            let layout = UiLayout::from_area(frame.area());

            // Reserve 1 line at the bottom of the main area for search, export, or filter bar.
            let needs_bottom_bar =
                app.search.active || app.export.active || app.filter.active;
            let (main_area, bottom_bar) = if needs_bottom_bar {
                let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
                    .split(layout.main);
                (chunks[0], Some(chunks[1]))
            } else {
                (layout.main, None)
            };

            app.last_main_area = main_area;
            app.last_status_area = layout.status;
            app.update_viewport_height(main_area.height as usize);

            ui::render_toolbar(frame, layout.toolbar, app.active_mode, &app.theme);

            // If a filter result is showing, render it instead of the normal view.
            if app.filter.showing_result {
                if let Some(ref result_view) = app.filter.result_view {
                    result_view.render(frame, main_area, &app.theme);
                }
            } else {
                app.active_view().render(frame, main_area, &app.theme);
            }

            // Bottom bar: filter input > search > export (mutually exclusive)
            if app.filter.active {
                if let Some(area) = bottom_bar {
                    render_filter_bar(frame, area, &app.filter, &app.theme);
                }
            } else if app.search.active {
                if let Some(area) = bottom_bar {
                    render_search_bar(frame, area, &app.search, &app.theme);
                }
            } else if app.export.active {
                if let Some(area) = bottom_bar {
                    render_export_bar(frame, area, &app.export, &app.theme);
                }
            }

            let status = if app.filter.showing_result {
                app.filter
                    .result_view
                    .as_ref()
                    .map(|v| v.status_info())
                    .unwrap_or_else(|| crate::views::StatusInfo {
                        cursor_path: "$".to_string(),
                        extra: None,
                    })
            } else {
                app.active_view().status_info()
            };
            let metadata = app.document.metadata();
            // Prepend filter indicator to flash message when showing results.
            let filter_indicator: Option<String> = if app.filter.showing_result {
                Some(format!("[Filter: {}]", app.filter.query))
            } else {
                None
            };
            let flash = app
                .flash_message
                .as_ref()
                .map(|(msg, _)| msg.as_str())
                .or_else(|| filter_indicator.as_deref());
            ui::render_status_bar(
                frame,
                layout.status,
                &status,
                metadata,
                flash,
                &app.theme,
            );

            // Help overlay (rendered last so it's on top)
            if app.show_help {
                ui::render_help_overlay(frame, frame.area(), &app.theme);
            }
        })?;

        if app.should_quit {
            break;
        }

        match events.next()? {
            AppEvent::Key(key) => {
                if app.show_help {
                    // Any key dismisses the help overlay
                    app.show_help = false;
                } else if app.filter.active {
                    handle_filter_key(&mut app, key);
                } else if app.filter.showing_result {
                    handle_filter_result_key(&mut app, key);
                } else if app.export.active {
                    handle_export_key(&mut app, key);
                } else if app.search.active {
                    handle_search_key(&mut app, key);
                } else {
                    let global_action = input::handle_global_key(key);
                    match global_action {
                        ViewAction::None => {
                            let action = app.active_view_mut().handle_key(key);
                            handle_action(&mut app, action);
                        }
                        action => {
                            handle_action(&mut app, action);
                        }
                    }
                }
            }
            AppEvent::Mouse(mouse) => {
                handle_mouse(&mut app, mouse);
            }
            AppEvent::Resize => {}
            AppEvent::Tick => {
                // Debounced search: run after the user stops typing
                if app.search.should_search() {
                    app.run_search();
                }

                // Decay flash message
                if let Some((_, ref mut ttl)) = app.flash_message {
                    if *ttl == 0 {
                        app.flash_message = None;
                    } else {
                        *ttl -= 1;
                    }
                }
            }
        }
    }

    Ok(())
}

fn handle_search_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.search.close();
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // Run search immediately if there are pending changes, then close
            if app.search.dirty {
                app.run_search();
            }
            app.search.close();
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            app.search.query.pop();
            app.search.mark_dirty();
        }
        (KeyModifiers::NONE, KeyCode::Char(c)) => {
            app.search.query.push(c);
            app.search.mark_dirty();
        }
        (KeyModifiers::CONTROL, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Down) => {
            app.search.next_hit();
            app.navigate_to_current_hit();
        }
        (KeyModifiers::CONTROL, KeyCode::Char('p')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.search.prev_hit();
            app.navigate_to_current_hit();
        }
        (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
            app.search.regex_mode = !app.search.regex_mode;
            app.search.mark_dirty();
        }
        _ => {}
    }
}

fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    use crossterm::event::MouseEventKind;
    let main_area = app.last_main_area;
    let status_area = app.last_status_area;

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.active_view_mut().handle_key(crossterm::event::KeyEvent::new(
                KeyCode::Up,
                KeyModifiers::NONE,
            ));
        }
        MouseEventKind::ScrollDown => {
            app.active_view_mut().handle_key(crossterm::event::KeyEvent::new(
                KeyCode::Down,
                KeyModifiers::NONE,
            ));
        }
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            // Breadcrumb click: clicking a path segment in the status bar navigates there.
            if mouse.row >= status_area.y
                && mouse.row < status_area.y + status_area.height
                && mouse.column >= status_area.x
                && mouse.column < status_area.x + status_area.width
                && app.active_mode == ViewMode::Tree
            {
                if let Some(node_id) = app.tree_view.selected_node_id() {
                    let path = app.document.path_of(node_id);
                    let ancestors = app.document.ancestors_of(node_id);
                    if let Some(seg_idx) = ui::breadcrumb_hit_test(&path, mouse.column, status_area.x) {
                        if let Some(&target) = ancestors.get(seg_idx) {
                            app.tree_view.navigate_to_node(target);
                        }
                    }
                }
                return;
            }

            // Main area click: select the clicked row.
            if mouse.column >= main_area.x
                && mouse.column < main_area.x + main_area.width
                && mouse.row >= main_area.y
                && mouse.row < main_area.y + main_area.height
            {
                let clicked_row = (mouse.row - main_area.y) as usize;
                app.click_row(clicked_row);
            }
        }
        _ => {}
    }
}

fn handle_action(app: &mut App, action: ViewAction) {
    match action {
        ViewAction::None => {}
        ViewAction::Quit => {
            app.should_quit = true;
        }
        ViewAction::SwitchView(mode) => {
            app.ensure_view(mode);
            app.active_mode = mode;
        }
        ViewAction::StartSearch => {
            app.search.open();
        }
        ViewAction::NextSearchHit => {
            app.search.next_hit();
            app.navigate_to_current_hit();
        }
        ViewAction::PrevSearchHit => {
            app.search.prev_hit();
            app.navigate_to_current_hit();
        }
        ViewAction::ToggleHelp => {
            app.show_help = !app.show_help;
        }
        ViewAction::StartExport => {
            app.export.active = true;
            app.export.filename = default_export_filename(&app.document);
        }
        ViewAction::OpenFilter => {
            app.filter.open();
        }
        ViewAction::ExpandStub(stub_id) => {
            app.expand_lazy_stub(stub_id);
        }
        ViewAction::CopyToClipboard(text) => {
            if let Some(ref mut cb) = app.clipboard {
                if cb.set_text(&text).is_ok() {
                    let preview = if text.chars().count() > 40 {
                        format!("{}...", crate::util::truncate_chars(&text, 37))
                    } else {
                        text
                    };
                    app.flash_message = Some((format!("Copied: {}", preview), 15));
                }
            }
        }
    }
}

fn handle_export_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.export.active = false;
            app.export.filename.clear();
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            perform_export(app);
            app.export.active = false;
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            app.export.filename.pop();
        }
        (KeyModifiers::NONE, KeyCode::Char(c)) => {
            app.export.filename.push(c);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Export helpers
// ---------------------------------------------------------------------------

fn default_export_filename(doc: &JsonDocument) -> String {
    doc.metadata()
        .source_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| format!("{}_export.json", s.to_string_lossy()))
        .unwrap_or_else(|| "export.json".to_string())
}

fn perform_export(app: &mut App) {
    use std::io::Write;
    let content = export_current_view(app);
    match std::fs::File::create(&app.export.filename) {
        Ok(mut f) => {
            if f.write_all(content.as_bytes()).is_ok() {
                app.flash_message =
                    Some((format!("Exported to {}", app.export.filename), 20));
            } else {
                app.flash_message = Some(("Export failed: write error".to_string(), 20));
            }
        }
        Err(e) => {
            app.flash_message = Some((format!("Export failed: {}", e), 20));
        }
    }
}

fn export_current_view(app: &App) -> String {
    match app.active_mode {
        ViewMode::Tree => {
            // Export the subtree rooted at the selected node, or the full document.
            let root_id = app
                .tree_view
                .selected_node_id()
                .unwrap_or_else(|| app.document.root());
            let value = raw::rebuild_serde_value(&app.document, root_id);
            serde_json::to_string_pretty(&value).unwrap_or_default()
        }
        _ => {
            let value = raw::rebuild_serde_value(&app.document, app.document.root());
            serde_json::to_string_pretty(&value).unwrap_or_default()
        }
    }
}

fn render_search_bar(
    frame: &mut ratatui::Frame,
    area: Rect,
    search: &SearchState,
    theme: &Theme,
) {
    let hit_info = if search.hits.is_empty() {
        if search.query.is_empty() {
            String::new()
        } else {
            " No matches".to_string()
        }
    } else {
        format!(" {}/{}", search.current_hit + 1, search.hits.len())
    };

    let mut spans = vec![
        Span::styled(
            " / ",
            Style::new()
                .fg(theme.toolbar_active_fg)
                .bg(theme.toolbar_active_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if search.regex_mode {
        spans.push(Span::styled(
            "[.*] ",
            Style::new()
                .fg(theme.toolbar_active_bg)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD),
        ));
    }

    spans.extend([
        Span::styled(
            search.query.clone(),
            Style::new().fg(theme.fg).bg(theme.bg),
        ),
        Span::styled(
            "█",
            Style::new().fg(theme.toolbar_active_bg).bg(theme.bg),
        ),
        Span::styled(
            hit_info,
            Style::new().fg(theme.fg_dim).bg(theme.bg),
        ),
    ]);

    let line = Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line)
        .style(Style::new().bg(theme.bg));
    frame.render_widget(paragraph, area);
}

fn render_export_bar(
    frame: &mut ratatui::Frame,
    area: Rect,
    export: &ExportState,
    theme: &Theme,
) {
    let spans = vec![
        Span::styled(
            " Export: ",
            Style::new()
                .fg(theme.toolbar_active_fg)
                .bg(theme.toolbar_active_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            export.filename.clone(),
            Style::new().fg(theme.fg).bg(theme.bg),
        ),
        Span::styled(
            "\u{2588}",
            Style::new().fg(theme.toolbar_active_bg).bg(theme.bg),
        ),
        Span::styled(
            "  [Enter] save  [Esc] cancel",
            Style::new().fg(theme.fg_dim).bg(theme.bg),
        ),
    ];

    let line = Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line).style(Style::new().bg(theme.bg));
    frame.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Filter key handlers
// ---------------------------------------------------------------------------

/// Handle keys while the filter input bar is open (typing mode).
fn handle_filter_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.filter.close_input();
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            run_filter(app);
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            app.filter.query.pop();
            // Clear any stale error as the user edits.
            app.filter.error = None;
        }
        (KeyModifiers::NONE, KeyCode::Char(c)) => {
            app.filter.query.push(c);
            app.filter.error = None;
        }
        _ => {}
    }
}

/// Handle keys while a filter result is displayed (navigation mode).
fn handle_filter_result_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.filter.close_result();
        }
        // Re-open the filter bar to refine the query.
        (KeyModifiers::NONE, KeyCode::Char(':')) => {
            app.filter.open();
        }
        _ => {
            // Delegate navigation keys to the result tree view.
            if let Some(ref mut view) = app.filter.result_view {
                view.handle_key(key);
            }
        }
    }
}

/// Parse and evaluate the current filter query, updating `app.filter`.
fn run_filter(app: &mut App) {
    let query = app.filter.query.trim().to_string();

    // Parse
    let expr = match crate::filter::parse::parse(&query) {
        Ok(e) => e,
        Err(e) => {
            app.filter.error = Some(e.to_string());
            return;
        }
    };

    // Rebuild the serde value from the document root
    let root_value = raw::rebuild_serde_value(&app.document, app.document.root());

    // Evaluate
    let results = match crate::filter::eval::eval(&root_value, &expr) {
        Ok(r) => r,
        Err(e) => {
            app.filter.error = Some(e.to_string());
            return;
        }
    };

    // Wrap multiple results in an array; a single result stands on its own.
    let result_value = if results.len() == 1 {
        results.into_iter().next().unwrap()
    } else {
        serde_json::Value::Array(results)
    };

    // Build a JsonDocument from the result value.
    let result_doc = Arc::new(crate::model::node::DocumentBuilder::from_serde_value(
        result_value,
        None,
        0,
        std::time::Duration::ZERO,
    ));

    let mut result_view = TreeView::new(Arc::clone(&result_doc));
    result_view.set_viewport_height(app.last_viewport_height);

    app.filter.result_doc = Some(result_doc);
    app.filter.result_view = Some(result_view);
    app.filter.error = None;
    app.filter.showing_result = true;
    app.filter.close_input();
}

fn render_filter_bar(
    frame: &mut ratatui::Frame,
    area: Rect,
    filter: &FilterState,
    theme: &Theme,
) {
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
            Style::new().fg(theme.string.fg.unwrap_or(theme.fg_dim)).bg(theme.bg),
        ));
    } else {
        spans.push(Span::styled(
            "  [Enter] run  [Esc] cancel",
            Style::new().fg(theme.fg_dim).bg(theme.bg),
        ));
    }

    let line = Line::from(spans);
    let paragraph = ratatui::widgets::Paragraph::new(line).style(Style::new().bg(theme.bg));
    frame.render_widget(paragraph, area);
}
