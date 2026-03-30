use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::model::node::DocumentMetadata;
use crate::theme::Theme;
use crate::util::format_count;
use crate::views::{StatusInfo, ViewMode};

/// Top-level layout: `[toolbar, main, status]`.
pub fn layout(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area)
}

/// Render the toolbar with view mode tabs.
pub fn render_toolbar(frame: &mut Frame, area: Rect, active_mode: ViewMode, theme: &Theme) {
    let mut spans: Vec<Span> = Vec::new();

    spans.push(Span::styled(" jlens ", theme.toolbar_brand_style));
    spans.push(Span::styled("  ", theme.toolbar_bg_style));

    for &mode in &ViewMode::ALL {
        let is_active = mode == active_mode;
        if is_active {
            spans.push(Span::styled(" \u{25cf} ", theme.toolbar_active_style));
            spans.push(Span::styled(
                format!("{} ", mode.label()),
                theme.toolbar_active_style,
            ));
        } else {
            spans.push(Span::styled(
                format!("  {} ", mode.label()),
                theme.fg_dim_style,
            ));
        }
    }

    // Right-align "? Help" hint
    let used_width: usize = spans
        .iter()
        .map(|s| crate::util::display_width(&s.content))
        .sum();
    let help_text = "? Help ";
    let help_width = crate::util::display_width(help_text);
    let total_width = area.width as usize;
    let padding = total_width.saturating_sub(used_width + help_width);
    spans.push(Span::styled(" ".repeat(padding), theme.toolbar_bg_style));
    spans.push(Span::styled(help_text, theme.fg_dim_style));

    let line = Line::from(spans).style(theme.toolbar_bg_style);
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
        use ratatui::style::Modifier;
        let line = Line::from(Span::styled(
            format!(" {} ", msg),
            theme.flash_style.add_modifier(Modifier::BOLD),
        ))
        .style(theme.flash_style);
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
        spans.push(Span::styled(format!(" {} ", extra), theme.status_fg_style));
    }

    // Calculate padding (use display width for correct Unicode handling)
    let left_width: usize = spans
        .iter()
        .map(|s| crate::util::display_width(&s.content))
        .sum();
    let right_width = crate::util::display_width(&right);
    let total_width = area.width as usize;
    let padding = total_width.saturating_sub(left_width + right_width);

    spans.push(Span::styled(" ".repeat(padding), theme.status_style));

    // Right: file info
    spans.push(Span::styled(
        format!(" {} ", file_name),
        theme.status_fg_style,
    ));
    spans.push(Span::styled(right, theme.status_dim_style));

    let line = Line::from(spans).style(theme.status_style);
    let paragraph = ratatui::widgets::Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Render the help overlay centered in the given area.
pub fn render_help_overlay(frame: &mut Frame, area: Rect, theme: &Theme) {
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
        ("z / Z", "Zoom in / out"),
        ("p", "Toggle preview"),
        ("+ / -", "Resize preview"),
        ("@", "Find path"),
        (":", "Filter (jq-like)"),
        ("y", "Copy value / subtree"),
        ("Y", "Copy JSON path"),
        ("e", "Expand all"),
        ("E", "Collapse all"),
        ("Ctrl+S", "Export to file"),
        ("q / Ctrl+C", "Quit"),
        ("?", "Toggle this help"),
    ];

    let content_height = help_lines.len() as u16 + 2 + 2; // +2 borders, +2 footer lines
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

    let mut lines: Vec<Line> = help_lines
        .iter()
        .map(|(key, desc)| {
            if desc.is_empty() && !key.is_empty() {
                // Section header
                Line::from(Span::styled(format!(" {}", key), theme.help_title_style))
            } else if key.is_empty() {
                Line::from("")
            } else {
                Line::from(vec![
                    Span::styled(format!("  {:<18}", key), theme.fg_bold_style),
                    Span::styled(desc.to_string(), theme.fg_dim_style),
                ])
            }
        })
        .collect();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press any key to close",
        theme.fg_dim_style,
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .title_style(theme.help_title_style)
        .border_style(theme.help_border_style)
        .style(theme.bg_style);

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay);
}

/// Parse a JSON path (e.g. `$.users[0].name`) into colored breadcrumb spans.
/// Non-path strings are rendered as plain text with the toolbar style.
fn breadcrumb_spans(path: &str, theme: &Theme) -> Vec<Span<'static>> {
    use ratatui::style::{Modifier, Style};
    // Extract raw colors from pre-computed composite styles.
    let bg = theme.toolbar_active_style.bg.unwrap_or(theme.bg);
    let toolbar_active_fg = theme.toolbar_active_style.fg.unwrap_or(theme.fg);
    let sep_fg = theme.toolbar_inactive_style.fg.unwrap_or(theme.fg_dim);
    let bold = Modifier::BOLD;

    if !path.starts_with('$') {
        return vec![Span::styled(
            format!(" {} ", path),
            theme.toolbar_brand_style,
        )];
    }

    let key_fg = theme.key.fg.unwrap_or(toolbar_active_fg);
    let idx_fg = theme.number.fg.unwrap_or(toolbar_active_fg);

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
                let fg = if bracket.contains('"') {
                    key_fg
                } else {
                    idx_fg
                };
                spans.push(Span::styled(bracket, Style::new().fg(fg).bg(bg)));
            }
            _ => {
                spans.push(Span::styled(
                    chars.next().unwrap().to_string(),
                    theme.toolbar_active_style,
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
        .thumb_style(theme.scrollbar_thumb_style)
        .track_style(theme.scrollbar_track_style);
    frame.render_stateful_widget(scrollbar, area, &mut state);
}
