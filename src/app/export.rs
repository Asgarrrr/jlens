use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::model::node::{JsonDocument, NodeId};
use crate::theme::Theme;
use crate::views::ViewMode;
use crate::views::raw;

pub(crate) struct ExportState {
    pub(crate) active: bool,
    pub(crate) filename: String,
}

pub(crate) enum ExportAction {
    None,
    Cancel,
    Confirm,
}

impl ExportState {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            filename: String::new(),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> ExportAction {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => ExportAction::Cancel,
            (KeyModifiers::NONE, KeyCode::Enter) => ExportAction::Confirm,
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.filename.pop();
                ExportAction::None
            }
            (KeyModifiers::NONE, KeyCode::Char(c)) => {
                self.filename.push(c);
                ExportAction::None
            }
            _ => ExportAction::None,
        }
    }
}

pub(crate) fn default_export_filename(doc: &JsonDocument) -> String {
    doc.metadata()
        .source_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| format!("{}_export.json", s.to_string_lossy()))
        .unwrap_or_else(|| "export.json".to_string())
}

pub(crate) fn perform_export(filename: &str, content: &str) -> Result<String, String> {
    use std::io::Write;
    let filename = filename.trim();
    if filename.is_empty() {
        return Err("Export failed: no filename".to_string());
    }
    match std::fs::File::create(filename) {
        Ok(mut f) => {
            f.write_all(content.as_bytes())
                .map_err(|e| format!("Export failed: {}", e))?;
            Ok(format!("Exported to {}", filename))
        }
        Err(e) => Err(format!("Export failed: {}", e)),
    }
}

pub(crate) fn export_current_view(
    document: &JsonDocument,
    active_mode: ViewMode,
    selected_node: Option<NodeId>,
) -> String {
    match active_mode {
        ViewMode::Tree => {
            let root_id = selected_node.unwrap_or_else(|| document.root());
            let value = raw::rebuild_serde_value(document, root_id);
            serde_json::to_string_pretty(&value).unwrap_or_default()
        }
        _ => {
            let value = raw::rebuild_serde_value(document, document.root());
            serde_json::to_string_pretty(&value).unwrap_or_default()
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

pub(crate) struct ExportBar<'a> {
    pub(crate) state: &'a ExportState,
    pub(crate) theme: &'a Theme,
}

impl Widget for ExportBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let export = self.state;
        let theme = self.theme;

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
        ratatui::widgets::Paragraph::new(line)
            .style(Style::new().bg(theme.bg))
            .render(area, buf);
    }
}
