pub mod graph;
pub mod path;
pub mod raw;
pub mod schema;
pub mod stats;
pub mod table;
pub mod tree;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::keymap::Action;
use crate::model::node::NodeId;
use crate::theme::Theme;

/// Information displayed in the status bar, provided by the active view.
pub(crate) struct StatusInfo {
    pub(crate) cursor_path: String,
}

/// Actions returned by views to communicate intent to the app layer.
pub(crate) enum ViewAction {
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
pub(crate) enum ViewMode {
    Tree,
    Table,
    Raw,
    Paths,
    Stats,
    Schema,
    Graph,
}

impl ViewMode {
    pub(crate) const ALL: [ViewMode; 7] = [
        ViewMode::Tree,
        ViewMode::Table,
        ViewMode::Raw,
        ViewMode::Paths,
        ViewMode::Stats,
        ViewMode::Schema,
        ViewMode::Graph,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            ViewMode::Tree => "Tree",
            ViewMode::Table => "Table",
            ViewMode::Raw => "Raw",
            ViewMode::Paths => "Paths",
            ViewMode::Stats => "Stats",
            ViewMode::Schema => "Schema",
            ViewMode::Graph => "Graph",
        }
    }

}

/// Trait implemented by all visualization modes.
pub(crate) trait View {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme);
    fn handle_action(&mut self, action: Action) -> ViewAction;
    fn status_info(&self) -> StatusInfo;
    fn set_viewport_height(&mut self, height: usize);
    /// Handle a mouse click on a row within the viewport.
    /// `row_in_viewport` is 0-based from the top of the visible area.
    fn click_row(&mut self, _row_in_viewport: usize) {}
}
