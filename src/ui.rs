use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::model::node::{JsonDocument, NodeId};
use crate::theme::Theme;
use crate::views::ViewMode;

/// Layout for diff mode: `[toolbar, main, status]`.
pub fn layout(area: Rect) -> [Rect; 3] {
    Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(area)
}

/// Build the main bordered block with tabs in the title and status in the footer.
pub fn build_main_block<'a>(
    active_mode: ViewMode,
    filter_active: bool,
    zoom_stack: &[NodeId],
    document: &JsonDocument,
    theme: &'a Theme,
) -> Block<'a> {
    // Title: tabs
    let mut title_spans = vec![
        Span::styled(" jlens ", theme.toolbar_brand_style),
        Span::raw(" "),
    ];
    for &mode in &ViewMode::ALL {
        if mode == active_mode && !filter_active {
            title_spans.push(Span::styled(format!(" \u{25cf} {} ", mode.label()), theme.toolbar_active_style));
        } else {
            title_spans.push(Span::styled(format!("  {}  ", mode.label()), theme.fg_dim_style));
        }
    }

    if !zoom_stack.is_empty() {
        let path = document.path_of(*zoom_stack.last().unwrap());
        title_spans.push(Span::styled(format!(" zoom:{path} "), theme.fg_dim_style));
    }

    if filter_active {
        title_spans.push(Span::styled(" \u{25cf} Filter ", theme.toolbar_active_style));
    }

    Block::bordered()
        .title(Line::from(title_spans))
        .border_style(theme.tree_guide_style)
        .style(theme.bg_style)
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
