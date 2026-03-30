mod diff;
mod export;
mod filter;
mod search;
mod terminal;

use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::event::AppEvent;
use crate::keymap::{Action, KeyMap};
use crate::model::lazy::LazyDocument;
use crate::model::node::{JsonDocument, NodeId};
use crate::parser;
use crate::search as search_mod;
use crate::search::SearchOptions;
use crate::theme::Theme;
use crate::ui;
use crate::views::path::PathView;
use crate::views::raw::RawView;
use crate::views::stats::StatsView;
use crate::views::table::TableView;
use crate::views::tree::TreeView;
use crate::views::{View, ViewAction, ViewMode};

use export::{ExportAction, ExportState};
use filter::{FilterAction, FilterState};
use search::{SearchAction, SearchState};

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct App {
    document: Arc<JsonDocument>,
    theme: Theme,
    active_mode: ViewMode,
    keymap: KeyMap,
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
    needs_redraw: bool,
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
            keymap: KeyMap::default(),
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
            needs_redraw: true,
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

    /// Expand a lazy stub node, updating the document and views.
    fn expand_lazy_stub(&mut self, stub_id: NodeId) {
        let Some(ref mut lazy) = self.lazy_doc else {
            return;
        };

        if let Err(e) = lazy.expand_node(stub_id) {
            self.flash_message = Some((format!("Expand failed: {}", e), 18));
            return;
        }

        // Rebuild document snapshot from the mutated arena.
        let doc = Arc::new(lazy.to_document());
        self.document = Arc::clone(&doc);
        self.tree_view.update_document(doc, Some(stub_id));

        // Invalidate lazily-constructed views so they rebuild with new data.
        self.raw_view = None;
        self.table_view = None;
        self.path_view = None;
        self.stats_view = None;

        // Update stub IDs.
        let stubs: HashSet<NodeId> = lazy.stub_ids().collect();
        self.tree_view.set_stub_ids(stubs);
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
        let views: [Option<&mut dyn View>; 6] = [
            Some(&mut self.tree_view),
            self.raw_view.as_mut().map(|v| v as &mut dyn View),
            self.table_view.as_mut().map(|v| v as &mut dyn View),
            self.path_view.as_mut().map(|v| v as &mut dyn View),
            self.stats_view.as_mut().map(|v| v as &mut dyn View),
            self.filter.result_view.as_mut().map(|v| v as &mut dyn View),
        ];
        for view in views.into_iter().flatten() {
            view.set_viewport_height(height);
        }
    }

    fn run_search(&mut self) {
        self.search.dirty = false;
        let opts = SearchOptions {
            regex_mode: self.search.regex_mode,
            ..Default::default()
        };
        self.search.hits = search_mod::search(&self.document, &self.search.query, &opts);
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
            let content = std::fs::read_to_string(path).unwrap_or_default();
            print_json_error(&path.display().to_string(), &content, line, column, &message);
            std::process::exit(1);
        }
        Err(e) => Err(e).with_context(|| format!("Failed to open {}", path.display())),
    }
}

/// Run reading JSON from stdin with progress indicator.
pub fn run_stdin(theme: Theme) -> Result<()> {
    use std::io::{Read, Write};

    const SPINNER: &[u8] = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".as_bytes();
    const SPINNER_CHARS: usize = 10;
    const PROGRESS_INTERVAL: usize = 256 * 1024; // update every 256KB

    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    let mut total = 0usize;
    let mut last_progress = 0usize;
    let stderr = std::io::stderr();

    loop {
        match std::io::stdin().read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                total += n;
                if total - last_progress >= PROGRESS_INTERVAL {
                    last_progress = total;
                    // Each braille char is 3 bytes UTF-8
                    let idx = (total / PROGRESS_INTERVAL) % SPINNER_CHARS;
                    let spin = &SPINNER[idx * 3..(idx + 1) * 3];
                    let mut err = stderr.lock();
                    let _ = write!(err, "\r\x1b[2m");
                    let _ = err.write_all(spin);
                    let _ = write!(err, " Reading stdin... ");
                    write_bytes_human(&mut err, total);
                    let _ = write!(err, "\x1b[0m");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e).context("Failed to read from stdin"),
        }
    }

    if total > 0 {
        let _ = write!(stderr.lock(), "\r\x1b[2K");
    }

    let text = String::from_utf8(buf).context("stdin is not valid UTF-8")?;
    let start = std::time::Instant::now();

    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(single_err) => match try_json_lines(&text) {
            Some(v) => v,
            None => {
                print_json_error("stdin", &text, single_err.line(), single_err.column(), &single_err.to_string());
                std::process::exit(1);
            }
        },
    };

    let parse_time = start.elapsed();
    let source_size = text.len() as u64;
    let document =
        crate::model::node::DocumentBuilder::from_serde_value(value, None, source_size, parse_time);
    run_with_document(Arc::new(document), None, theme)
}

/// Try parsing input as JSON Lines (one JSON value per line).
/// Quick-checks the first non-empty line before committing to a full parse.
fn try_json_lines(text: &str) -> Option<serde_json::Value> {
    let first = text.lines().find(|l| !l.trim().is_empty())?;
    let first_trimmed = first.trim();
    // JSON Lines entries are typically objects or arrays
    if !(first_trimmed.starts_with('{') || first_trimmed.starts_with('[')) {
        return None;
    }
    // Verify the first line parses independently — if not, this isn't JSONL
    serde_json::from_str::<serde_json::Value>(first_trimmed).ok()?;

    let values: Result<Vec<_>, _> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(serde_json::from_str)
        .collect();

    match values {
        Ok(v) if v.len() >= 2 => Some(serde_json::Value::Array(v)),
        _ => None,
    }
}

fn write_bytes_human(w: &mut impl std::io::Write, bytes: usize) {
    if bytes < 1024 {
        let _ = write!(w, "{} B", bytes);
    } else if bytes < 1024 * 1024 {
        let _ = write!(w, "{:.1} KB", bytes as f64 / 1024.0);
    } else if bytes < 1024 * 1024 * 1024 {
        let _ = write!(w, "{:.1} MB", bytes as f64 / (1024.0 * 1024.0));
    } else {
        let _ = write!(w, "{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0));
    }
}

fn print_json_error(source: &str, content: &str, line: usize, column: usize, message: &str) {
    eprintln!("\x1b[1;31merror\x1b[0m: invalid JSON from \x1b[1m{}\x1b[0m", source);
    eprintln!("  --> line {}, column {}", line, column);
    eprintln!("  {}", message);
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
    const TICK: Duration = Duration::from_millis(33);

    loop {
        if app.needs_redraw {
            app.needs_redraw = false;
            terminal.draw(|frame| {
                let [toolbar, main_area_full, status] = ui::layout(frame.area());

                // Reserve 1 line at the bottom of the main area for search, export, or filter bar.
                let needs_bottom_bar = app.search.active || app.export.active || app.filter.active;
                let (main_area, bottom_bar) = if needs_bottom_bar {
                    let [main, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
                        .areas(main_area_full);
                    (main, Some(bar))
                } else {
                    (main_area_full, None)
                };

                app.last_main_area = main_area;
                app.last_status_area = status;
                app.update_viewport_height(main_area.height as usize);

                ui::render_toolbar(frame, toolbar, app.active_mode, &app.theme);

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
                        frame.render_widget(
                            filter::FilterBar {
                                state: &app.filter,
                                theme: &app.theme,
                            },
                            area,
                        );
                    }
                } else if app.search.active {
                    if let Some(area) = bottom_bar {
                        frame.render_widget(
                            search::SearchBar {
                                state: &app.search,
                                theme: &app.theme,
                            },
                            area,
                        );
                    }
                } else if app.export.active
                    && let Some(area) = bottom_bar
                {
                    frame.render_widget(
                        export::ExportBar {
                            state: &app.export,
                            theme: &app.theme,
                        },
                        area,
                    );
                }

                let status_info = if app.filter.showing_result {
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
                    .or(filter_indicator.as_deref());
                ui::render_status_bar(frame, status, &status_info, metadata, flash, &app.theme);

                // Help overlay (rendered last so it's on top)
                if app.show_help {
                    ui::render_help_overlay(frame, frame.area(), &app.theme);
                }
            })?;
        }

        if app.should_quit {
            break;
        }

        match crate::event::poll(TICK)? {
            AppEvent::Key(key) => {
                handle_key(&mut app, key);
                app.needs_redraw = true;
            }
            AppEvent::Mouse(mouse) => {
                handle_mouse(&mut app, mouse);
                app.needs_redraw = true;
            }
            AppEvent::Resize => {
                app.needs_redraw = true;
            }
            AppEvent::Tick => {
                // Debounced search: run after the user stops typing
                if app.search.should_search() {
                    app.run_search();
                    app.needs_redraw = true;
                }

                // Decay flash message
                if let Some((_, ref mut ttl)) = app.flash_message {
                    if *ttl == 0 {
                        app.flash_message = None;
                    } else {
                        *ttl -= 1;
                    }
                    app.needs_redraw = true;
                }
            }
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) {
    if app.show_help {
        app.show_help = false;
        return;
    }

    if app.filter.active {
        match app.filter.handle_input_key(key) {
            FilterAction::CloseInput => app.filter.close_input(),
            FilterAction::RunFilter => {
                filter::run_filter(&mut app.filter, &app.document, app.last_viewport_height);
            }
            FilterAction::None
            | FilterAction::CloseResult
            | FilterAction::ReopenInput
            | FilterAction::DelegateToResult(_) => {}
        }
        return;
    }

    if app.filter.showing_result {
        match app.filter.handle_result_key(key) {
            FilterAction::CloseResult => app.filter.close_result(),
            FilterAction::ReopenInput => app.filter.open(),
            FilterAction::DelegateToResult(k) => {
                if let Some(action) = app.keymap.resolve(&k) {
                    let view_action = if let Some(ref mut view) = app.filter.result_view {
                        view.handle_action(action)
                    } else {
                        ViewAction::None
                    };
                    handle_action(app, view_action);
                }
            }
            FilterAction::None | FilterAction::CloseInput | FilterAction::RunFilter => {}
        }
        return;
    }

    if app.export.active {
        match app.export.handle_key(key) {
            ExportAction::Cancel => {
                app.export.active = false;
                app.export.filename.clear();
            }
            ExportAction::Confirm => {
                let content = export::export_current_view(
                    &app.document,
                    app.active_mode,
                    app.tree_view.selected_node_id(),
                );
                let result = export::perform_export(&app.export.filename, &content);
                let msg = match result {
                    Ok(m) | Err(m) => m,
                };
                app.flash_message = Some((msg, 60));
                app.export.active = false;
            }
            ExportAction::None => {}
        }
        return;
    }

    if app.search.active {
        match app.search.handle_key(key) {
            SearchAction::Close => app.search.close(),
            SearchAction::RunSearchAndClose => {
                app.run_search();
                app.search.close();
            }
            SearchAction::Navigate => app.navigate_to_current_hit(),
            SearchAction::QueryChanged | SearchAction::ToggleRegex | SearchAction::None => {}
        }
        return;
    }

    if let Some(action) = app.keymap.resolve(&key) {
        let view_action = dispatch_action(app, action);
        handle_action(app, view_action);
    }
}

fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    use crossterm::event::MouseEventKind;
    let main_area = app.last_main_area;
    let status_area = app.last_status_area;

    /// Dispatch a scroll action to the currently visible view,
    /// routing the returned `ViewAction` through `handle_action`.
    fn scroll_view(app: &mut App, action: Action) {
        let view_action = if app.filter.showing_result {
            app.filter
                .result_view
                .as_mut()
                .map(|v| v.handle_action(action))
                .unwrap_or(ViewAction::None)
        } else {
            app.active_view_mut().handle_action(action)
        };
        handle_action(app, view_action);
    }

    match mouse.kind {
        MouseEventKind::ScrollUp => scroll_view(app, Action::MoveUp),
        MouseEventKind::ScrollDown => scroll_view(app, Action::MoveDown),
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            // Breadcrumb click: clicking a path segment in the status bar navigates there.
            if !app.filter.showing_result
                && mouse.row >= status_area.y
                && mouse.row < status_area.y + status_area.height
                && mouse.column >= status_area.x
                && mouse.column < status_area.x + status_area.width
                && app.active_mode == ViewMode::Tree
            {
                if let Some(node_id) = app.tree_view.selected_node_id() {
                    let path = app.document.path_of(node_id);
                    let ancestors = app.document.ancestors_of(node_id);
                    if let Some(seg_idx) =
                        ui::breadcrumb_hit_test(&path, mouse.column, status_area.x)
                        && let Some(&target) = ancestors.get(seg_idx)
                    {
                        app.tree_view.navigate_to_node(target);
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
                if app.filter.showing_result {
                    if let Some(ref mut v) = app.filter.result_view {
                        v.click_row(clicked_row);
                    }
                } else {
                    app.click_row(clicked_row);
                }
            }
        }
        _ => {}
    }
}

/// Convert a semantic `Action` to a `ViewAction`.
///
/// Global actions (Quit, SwitchView, StartSearch, etc.) are handled here
/// directly. View-local actions (MoveUp, ToggleExpand, etc.) are forwarded
/// to the active view.
fn dispatch_action(app: &mut App, action: Action) -> ViewAction {
    match action {
        Action::Quit => ViewAction::Quit,
        Action::SwitchView(n) => {
            let mode = match n {
                1 => crate::views::ViewMode::Tree,
                2 => crate::views::ViewMode::Table,
                3 => crate::views::ViewMode::Raw,
                4 => crate::views::ViewMode::Paths,
                5 => crate::views::ViewMode::Stats,
                _ => return ViewAction::None,
            };
            ViewAction::SwitchView(mode)
        }
        Action::StartSearch => ViewAction::StartSearch,
        Action::NextSearchHit => ViewAction::NextSearchHit,
        Action::PrevSearchHit => ViewAction::PrevSearchHit,
        Action::ToggleHelp => ViewAction::ToggleHelp,
        Action::StartExport => ViewAction::StartExport,
        Action::OpenFilter => ViewAction::OpenFilter,
        // All other actions are view-local
        other => app.active_view_mut().handle_action(other),
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
            app.export.filename = export::default_export_filename(&app.document);
        }
        ViewAction::OpenFilter => {
            app.filter.open();
        }
        ViewAction::ExpandStub(stub_id) => {
            app.expand_lazy_stub(stub_id);
        }
        ViewAction::CopyToClipboard(text) => {
            if let Some(ref mut cb) = app.clipboard
                && cb.set_text(&text).is_ok()
            {
                let preview = if text.chars().count() > 40 {
                    format!("{}...", crate::util::truncate_chars(&text, 37))
                } else {
                    text
                };
                app.flash_message = Some((format!("Copied: {}", preview), 45));
            }
        }
    }
}
