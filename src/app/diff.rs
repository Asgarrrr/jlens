use std::io;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::diff::algo;
use crate::diff::view::DiffView;
use crate::event::{AppEvent, EventReader};
use crate::parser;
use crate::theme::Theme;
use crate::ui::{self, UiLayout};
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
    let events = EventReader::new(Duration::from_millis(100));
    let mut show_help = false;
    let mut should_quit = false;
    let mut last_main_area = Rect::default();

    loop {
        terminal.draw(|frame| {
            let layout = UiLayout::from_area(frame.area());
            let main_area = layout.main;
            last_main_area = main_area;

            diff_view.set_viewport_height(main_area.height as usize);

            frame.render_widget(
                DiffToolbar { title, theme: &theme },
                layout.toolbar,
            );
            diff_view.render(frame, main_area, &theme);

            let status = diff_view.status_info();
            let stats = diff_view.stats();
            let stats_str = format!(
                " +{} added  -{} removed  ~{} modified ",
                stats.added, stats.removed, stats.modified
            );
            frame.render_widget(
                DiffStatusBar { status: &status, stats_str: &stats_str, theme: &theme },
                layout.status,
            );

            if show_help {
                ui::render_help_overlay(frame, frame.area(), &theme);
            }
        })?;

        if should_quit {
            break;
        }

        match events.next()? {
            AppEvent::Key(key) => {
                if show_help {
                    show_help = false;
                } else {
                    let action = diff_view.handle_key(key);
                    match action {
                        ViewAction::Quit => should_quit = true,
                        ViewAction::ToggleHelp => show_help = !show_help,
                        _ => {}
                    }
                }
            }
            AppEvent::Mouse(mouse) => {
                use crossterm::event::MouseEventKind;
                match mouse.kind {
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let code = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                            crossterm::event::KeyCode::Up
                        } else {
                            crossterm::event::KeyCode::Down
                        };
                        let action = diff_view.handle_key(crossterm::event::KeyEvent::new(
                            code,
                            crossterm::event::KeyModifiers::NONE,
                        ));
                        match action {
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
                Style::new()
                    .fg(self.theme.toolbar_active_fg)
                    .bg(self.theme.toolbar_active_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} ", self.title),
                Style::new()
                    .fg(self.theme.toolbar_fg)
                    .bg(self.theme.toolbar_bg),
            ),
        ];
        let line = Line::from(spans).style(Style::new().bg(self.theme.toolbar_bg));
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
                Style::new()
                    .fg(self.theme.toolbar_active_fg)
                    .bg(self.theme.toolbar_active_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " ".repeat(padding),
                Style::new().bg(self.theme.status_bg),
            ),
            Span::styled(
                self.stats_str.to_string(),
                Style::new()
                    .fg(self.theme.status_fg)
                    .bg(self.theme.status_bg),
            ),
        ];
        let line = Line::from(spans).style(Style::new().bg(self.theme.status_bg));
        ratatui::widgets::Paragraph::new(line).render(area, buf);
    }
}
