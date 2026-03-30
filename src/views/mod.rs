pub mod path;
pub mod raw;
pub mod stats;
pub mod table;
pub mod tree;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::keymap::Action;
use crate::model::node::NodeId;
use crate::theme::Theme;

/// Information displayed in the status bar, provided by the active view.
pub struct StatusInfo {
    pub cursor_path: String,
    pub extra: Option<String>,
}

/// Actions returned by views to communicate intent to the app layer.
pub enum ViewAction {
    None,
    Quit,
    SwitchView(ViewMode),
    StartSearch,
    NextSearchHit,
    PrevSearchHit,
    CopyToClipboard(String),
    ToggleHelp,
    StartExport,
    OpenFilter,
    /// The tree view wants to expand a lazy stub — App should trigger expansion.
    ExpandStub(NodeId),
}

/// Available visualization modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Tree,
    Table,
    Raw,
    Paths,
    Stats,
}

impl ViewMode {
    pub const ALL: [ViewMode; 5] = [
        ViewMode::Tree,
        ViewMode::Table,
        ViewMode::Raw,
        ViewMode::Paths,
        ViewMode::Stats,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Tree => "Tree",
            ViewMode::Table => "Table",
            ViewMode::Raw => "Raw",
            ViewMode::Paths => "Paths",
            ViewMode::Stats => "Stats",
        }
    }

    pub fn shortcut(self) -> char {
        match self {
            ViewMode::Tree => '1',
            ViewMode::Table => '2',
            ViewMode::Raw => '3',
            ViewMode::Paths => '4',
            ViewMode::Stats => '5',
        }
    }
}

/// Trait implemented by all visualization modes.
pub trait View {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme);
    fn handle_action(&mut self, action: Action) -> ViewAction;
    fn status_info(&self) -> StatusInfo;
    fn set_viewport_height(&mut self, height: usize);
    /// Handle a mouse click on a row within the viewport.
    /// `row_in_viewport` is 0-based from the top of the visible area.
    fn click_row(&mut self, _row_in_viewport: usize) {}
}
