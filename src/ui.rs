use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use crate::model::node::DocumentMetadata;
use crate::theme::Theme;
use crate::views::{StatusInfo, ViewMode};

/// Top-level layout: toolbar (1 line) + main view + status bar (1 line).
pub struct UiLayout {
    pub toolbar: Rect,
    pub main: Rect,
    pub status: Rect,
}

impl UiLayout {
    pub fn from_area(area: Rect) -> Self {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        Self {
            toolbar: chunks[0],
            main: chunks[1],
            status: chunks[2],
        }
    }
}

/// Render the toolbar with view mode tabs.
pub fn render_toolbar(
    frame: &mut Frame,
    area: Rect,
    active_mode: ViewMode,
    theme: &Theme,
) {
    let mut spans: Vec<Span> = Vec::new();

    spans.push(Span::styled(
        " jlens ",
        Style::new()
            .fg(theme.toolbar_active_fg)
            .bg(theme.toolbar_active_bg)
            .add_modifier(ratatui::style::Modifier::BOLD),
    ));

    spans.push(Span::styled(
        " ",
        Style::new().bg(theme.toolbar_bg),
    ));

    for mode in ViewMode::ALL {
        let is_active = mode == active_mode;
        let label = format!(" {}: {} ", mode.shortcut(), mode.label());

        if is_active {
            spans.push(Span::styled(
                label,
                Style::new()
                    .fg(theme.toolbar_active_fg)
                    .bg(theme.toolbar_active_bg)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                label,
                Style::new()
                    .fg(theme.toolbar_fg)
                    .bg(theme.toolbar_bg),
            ));
        }
    }

    // Fill remaining width
    let line = Line::from(spans).style(Style::new().bg(theme.toolbar_bg));
    let paragraph = ratatui::widgets::Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Render the status bar with cursor path and document info.
pub fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    status: &StatusInfo,
    metadata: &DocumentMetadata,
    flash_message: Option<&str>,
    theme: &Theme,
) {
    // Flash message takes over the entire status bar briefly
    if let Some(msg) = flash_message {
        let line = Line::from(Span::styled(
            format!(" {} ", msg),
            Style::new()
                .fg(theme.flash_fg)
                .bg(theme.flash_bg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))
        .style(Style::new().bg(theme.flash_bg));
        let paragraph = ratatui::widgets::Paragraph::new(line);
        frame.render_widget(paragraph, area);
        return;
    }
    let file_name = metadata
        .source_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "stdin".to_string());

    let size_str = humansize::format_size(metadata.source_size, humansize::BINARY);
    let nodes_str = format_count(metadata.total_nodes);
    let depth_str = metadata.max_depth.to_string();
    let parse_ms = metadata.parse_time.as_millis();

    let right = format!(
        " {} | {} nodes | d:{} | {}ms ",
        size_str, nodes_str, depth_str, parse_ms,
    );

    let mut spans = Vec::new();

    // Left: cursor path (rendered as colored breadcrumb for JSON paths)
    spans.extend(breadcrumb_spans(&status.cursor_path, theme));

    if let Some(ref extra) = status.extra {
        spans.push(Span::styled(
            format!(" {} ", extra),
            Style::new().fg(theme.status_fg).bg(theme.status_bg),
        ));
    }

    // Calculate padding (use display width for correct Unicode handling)
    let left_width: usize = spans.iter().map(|s| crate::util::display_width(&s.content)).sum();
    let right_width = crate::util::display_width(&right);
    let total_width = area.width as usize;
    let padding = total_width.saturating_sub(left_width + right_width);

    spans.push(Span::styled(
        " ".repeat(padding),
        Style::new().bg(theme.status_bg),
    ));

    // Right: file info
    spans.push(Span::styled(
        format!(" {} ", file_name),
        Style::new().fg(theme.status_fg).bg(theme.status_bg),
    ));
    spans.push(Span::styled(
        right,
        Style::new().fg(theme.fg_dim).bg(theme.status_bg),
    ));

    let line = Line::from(spans).style(Style::new().bg(theme.status_bg));
    let paragraph = ratatui::widgets::Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Render the help overlay centered in the given area.
pub fn render_help_overlay(frame: &mut Frame, area: Rect, theme: &Theme) {
    use ratatui::style::Modifier;
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    let help_lines: &[(&str, &str)] = &[
        ("Navigation", ""),
        ("j / Down", "Move down"),
        ("k / Up", "Move up"),
        ("h / Left", "Collapse / go to parent"),
        ("l / Right", "Expand"),
        ("Enter / Space", "Toggle expand/collapse"),
        ("Ctrl+D / PgDn", "Page down"),
        ("Ctrl+U / PgUp", "Page up"),
        ("Home", "Go to top"),
        ("G / End", "Go to bottom"),
        ("Mouse scroll", "Scroll up/down"),
        ("Mouse click", "Select row / breadcrumb"),
        ("", ""),
        ("Views", ""),
        ("1-5", "Switch view mode"),
        ("Tab / Shift+Tab", "Cycle sort column (table)"),
        ("s", "Toggle sort direction (table)"),
        ("", ""),
        ("Search", ""),
        ("/ or Ctrl+F", "Open search"),
        ("Ctrl+R", "Toggle regex mode"),
        ("n", "Next match"),
        ("N", "Previous match"),
        ("Esc / Enter", "Close search"),
        ("", ""),
        ("Actions", ""),
        ("y", "Copy value / subtree"),
        ("Y", "Copy JSON path"),
        ("e", "Expand all"),
        ("E", "Collapse all"),
        ("Ctrl+S", "Export to file"),
        ("q / Ctrl+C", "Quit"),
        ("?", "Toggle this help"),
    ];

    let content_height = help_lines.len() as u16 + 2; // +2 for borders
    let content_width = 48u16;

    let x = area.x + area.width.saturating_sub(content_width) / 2;
    let y = area.y + area.height.saturating_sub(content_height) / 2;
    let overlay = Rect::new(
        x,
        y,
        content_width.min(area.width),
        content_height.min(area.height),
    );

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay);

    let lines: Vec<Line> = help_lines
        .iter()
        .map(|(key, desc)| {
            if desc.is_empty() && !key.is_empty() {
                // Section header
                Line::from(Span::styled(
                    format!(" {}", key),
                    Style::new()
                        .fg(theme.toolbar_active_bg)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if key.is_empty() {
                Line::from("")
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("  {:<18}", key),
                        Style::new().fg(theme.fg).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(desc.to_string(), Style::new().fg(theme.fg_dim)),
                ])
            }
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .title_style(Style::new().fg(theme.toolbar_active_bg).add_modifier(Modifier::BOLD))
        .border_style(Style::new().fg(theme.tree_guide))
        .style(Style::new().bg(theme.bg));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay);
}

/// Parse a JSON path (e.g. `$.users[0].name`) into colored breadcrumb spans.
/// Non-path strings are rendered as plain text with the toolbar style.
fn breadcrumb_spans(path: &str, theme: &Theme) -> Vec<Span<'static>> {
    let bg = theme.toolbar_active_bg;
    let bold = ratatui::style::Modifier::BOLD;

    if !path.starts_with('$') {
        return vec![Span::styled(
            format!(" {} ", path),
            Style::new().fg(theme.toolbar_active_fg).bg(bg).add_modifier(bold),
        )];
    }

    let key_fg = theme.key.fg.unwrap_or(theme.toolbar_active_fg);
    let idx_fg = theme.number.fg.unwrap_or(theme.toolbar_active_fg);
    let sep_fg = theme.toolbar_fg;

    let mut spans = vec![Span::styled(" ", Style::new().bg(bg))];
    let mut chars = path.chars().peekable();

    // "$" root
    if chars.peek() == Some(&'$') {
        chars.next();
        spans.push(Span::styled("$", Style::new().fg(sep_fg).bg(bg)));
    }

    while let Some(&ch) = chars.peek() {
        match ch {
            '.' => {
                chars.next();
                spans.push(Span::styled(".", Style::new().fg(sep_fg).bg(bg)));
                let mut key = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '.' || c == '[' {
                        break;
                    }
                    key.push(chars.next().unwrap());
                }
                if !key.is_empty() {
                    spans.push(Span::styled(
                        key,
                        Style::new().fg(key_fg).bg(bg).add_modifier(bold),
                    ));
                }
            }
            '[' => {
                let mut bracket = String::new();
                while let Some(&c) = chars.peek() {
                    bracket.push(chars.next().unwrap());
                    if c == ']' {
                        break;
                    }
                }
                let fg = if bracket.contains('"') { key_fg } else { idx_fg };
                spans.push(Span::styled(bracket, Style::new().fg(fg).bg(bg)));
            }
            _ => {
                spans.push(Span::styled(
                    chars.next().unwrap().to_string(),
                    Style::new().fg(theme.toolbar_active_fg).bg(bg),
                ));
            }
        }
    }

    spans.push(Span::styled(" ", Style::new().bg(bg)));
    spans
}

/// Determine which breadcrumb segment index was clicked given the mouse x and area x.
/// Returns the 0-based segment index (0 = root "$", 1 = first key/index, etc.)
pub fn breadcrumb_hit_test(path: &str, click_x: u16, area_x: u16) -> Option<usize> {
    if !path.starts_with('$') {
        return None;
    }

    let x = (click_x.saturating_sub(area_x)) as usize;
    let mut offset = 1usize; // leading " "
    let mut segment_idx = 0usize;
    let mut chars = path.chars().peekable();

    // "$"
    if chars.peek() == Some(&'$') {
        chars.next();
        if x >= offset && x < offset + 1 {
            return Some(0);
        }
        offset += 1;
        segment_idx = 1;
    }

    while let Some(&ch) = chars.peek() {
        let seg_start = offset;
        match ch {
            '.' => {
                chars.next();
                offset += 1; // "."
                let mut key = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '.' || c == '[' {
                        break;
                    }
                    key.push(chars.next().unwrap());
                }
                offset += crate::util::display_width(&key);
                if x >= seg_start && x < offset {
                    return Some(segment_idx);
                }
                segment_idx += 1;
            }
            '[' => {
                let mut bracket = String::new();
                while let Some(&c) = chars.peek() {
                    bracket.push(chars.next().unwrap());
                    if c == ']' {
                        break;
                    }
                }
                offset += crate::util::display_width(&bracket);
                if x >= seg_start && x < offset {
                    return Some(segment_idx);
                }
                segment_idx += 1;
            }
            _ => {
                chars.next();
                offset += 1;
            }
        }
    }

    None
}

/// Render a vertical scrollbar using ratatui's native widget.
/// Call this from any view's `render` when content exceeds the viewport.
pub fn render_scrollbar(
    frame: &mut Frame,
    area: Rect,
    content_length: usize,
    position: usize,
    theme: &Theme,
) {
    let mut state = ScrollbarState::default()
        .content_length(content_length)
        .position(position);
    let scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .begin_symbol(None)
        .end_symbol(None)
        .thumb_style(Style::new().fg(theme.fg_dim))
        .track_style(Style::new().fg(theme.tree_guide));
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

/// Format a number with thousand separators (e.g. 45231 → "45,231").
fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_count_small() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(42), "42");
        assert_eq!(format_count(999), "999");
    }

    #[test]
    fn format_count_thousands() {
        assert_eq!(format_count(1000), "1,000");
        assert_eq!(format_count(45231), "45,231");
        assert_eq!(format_count(1000000), "1,000,000");
    }
}
