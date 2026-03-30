use std::io;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::diff::algo;
use crate::diff::view::DiffView;
use crate::event::AppEvent;
use crate::keymap::KeyMap;
use crate::parser;
use crate::theme::Theme;
use crate::ui;
use crate::views::raw;
use crate::views::{View, ViewAction, StatusInfo};

/// Run a structural diff between two JSON files, showing the result in the TUI.
pub fn run_diff(path_a: &Path, path_b: &Path, theme: Theme) -> Result<()> {
    let doc_a = parser::parse_file(path_a)
        .with_context(|| format!("Failed to open {}", path_a.display()))?;
    let doc_b = parser::parse_file(path_b)
        .with_context(|| format!("Failed to open {}", path_b.display()))?;

    let val_a = raw::rebuild_serde_value(&doc_a, doc_a.root());
    let val_b = raw::rebuild_serde_value(&doc_b, doc_b.root());
    let diff_result = algo::diff(&val_a, &val_b);
    let diff_view = DiffView::new(diff_result);

    let name_a = path_a
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_a.display().to_string());
    let name_b = path_b
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_b.display().to_string());
    let title = format!("Diff: {} \u{2194} {}", name_a, name_b);

    super::terminal::with_terminal(|t| run_diff_app(t, diff_view, &title, theme))
}

fn run_diff_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut diff_view: DiffView,
    title: &str,
    theme: Theme,
) -> Result<()> {
    const TICK: Duration = Duration::from_millis(100);
    let keymap = KeyMap::default_map();
    let mut show_help = false;
    let mut should_quit = false;
    let mut last_main_area = Rect::default();

    loop {
        terminal.draw(|frame| {
            let [toolbar, main_area, status_area] = ui::layout(frame.area());
            last_main_area = main_area;

            diff_view.set_viewport_height(main_area.height as usize);

            frame.render_widget(DiffToolbar { title, theme: &theme }, toolbar);
            diff_view.render(frame, main_area, &theme);

            let status = diff_view.status_info();
            let stats = diff_view.stats();
            let stats_str = format!(
                " +{} added  -{} removed  ~{} modified ",
                stats.added, stats.removed, stats.modified
            );
            frame.render_widget(
                DiffStatusBar { status: &status, stats_str: &stats_str, theme: &theme },
                status_area,
            );

            if show_help {
                ui::render_help_overlay(frame, frame.area(), &theme);
            }
        })?;

        if should_quit {
            break;
        }

        match crate::event::poll(TICK)? {
            AppEvent::Key(key) => {
                if show_help {
                    show_help = false;
                } else if let Some(action) = keymap.resolve(&key) {
                    let view_action = diff_view.handle_action(action);
                    match view_action {
                        ViewAction::Quit => should_quit = true,
                        ViewAction::ToggleHelp => show_help = !show_help,
                        _ => {}
                    }
                }
            }
            AppEvent::Mouse(mouse) => {
                use crossterm::event::MouseEventKind;
                use crate::keymap::Action;
                match mouse.kind {
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let scroll_action = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                            Action::MoveUp
                        } else {
                            Action::MoveDown
                        };
                        let view_action = diff_view.handle_action(scroll_action);
                        match view_action {
                            ViewAction::Quit => should_quit = true,
                            ViewAction::ToggleHelp => show_help = !show_help,
                            _ => {}
                        }
                    }
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        if mouse.column >= last_main_area.x
                            && mouse.column < last_main_area.x + last_main_area.width
                            && mouse.row >= last_main_area.y
                            && mouse.row < last_main_area.y + last_main_area.height
                        {
                            let clicked = (mouse.row - last_main_area.y) as usize;
                            diff_view.click_row(clicked);
                        }
                    }
                    _ => {}
                }
            }
            AppEvent::Resize => {}
            AppEvent::Tick => {}
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

struct DiffToolbar<'a> {
    title: &'a str,
    theme: &'a Theme,
}

impl Widget for DiffToolbar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let spans = vec![
            Span::styled(
                " jlens ",
                self.theme.toolbar_brand_style,
            ),
            Span::styled(
                format!("  {} ", self.title),
                self.theme.toolbar_inactive_style,
            ),
        ];
        let line = Line::from(spans).style(self.theme.toolbar_bg_style);
        ratatui::widgets::Paragraph::new(line).render(area, buf);
    }
}

struct DiffStatusBar<'a> {
    status: &'a StatusInfo,
    stats_str: &'a str,
    theme: &'a Theme,
}

impl Widget for DiffStatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let left = format!(" {} ", self.status.cursor_path);
        let right_width = crate::util::display_width(self.stats_str);
        let left_width = crate::util::display_width(&left);
        let total_w = area.width as usize;
        let padding = total_w.saturating_sub(left_width + right_width);

        let spans = vec![
            Span::styled(
                left,
                self.theme.toolbar_brand_style,
            ),
            Span::styled(
                " ".repeat(padding),
                self.theme.status_style,
            ),
            Span::styled(
                self.stats_str.to_string(),
                self.theme.status_fg_style,
            ),
        ];
        let line = Line::from(spans).style(self.theme.status_style);
        ratatui::widgets::Paragraph::new(line).render(area, buf);
    }
}
