use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::views::ViewAction;

/// Global keybindings handled at the app level (before views).
pub fn handle_global_key(key: KeyEvent) -> ViewAction {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('q')) => ViewAction::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => ViewAction::Quit,
        (KeyModifiers::NONE, KeyCode::Char('1')) => ViewAction::SwitchView(crate::views::ViewMode::Tree),
        (KeyModifiers::NONE, KeyCode::Char('2')) => ViewAction::SwitchView(crate::views::ViewMode::Table),
        (KeyModifiers::NONE, KeyCode::Char('3')) => ViewAction::SwitchView(crate::views::ViewMode::Raw),
        (KeyModifiers::NONE, KeyCode::Char('4')) => ViewAction::SwitchView(crate::views::ViewMode::Paths),
        (KeyModifiers::NONE, KeyCode::Char('5')) => ViewAction::SwitchView(crate::views::ViewMode::Stats),
        (KeyModifiers::NONE, KeyCode::Char('/')) => ViewAction::StartSearch,
        (KeyModifiers::CONTROL, KeyCode::Char('f')) => ViewAction::StartSearch,
        (KeyModifiers::NONE, KeyCode::Char('n')) => ViewAction::NextSearchHit,
        (KeyModifiers::SHIFT, KeyCode::Char('N')) => ViewAction::PrevSearchHit,
        (KeyModifiers::SHIFT, KeyCode::Char('?')) => ViewAction::ToggleHelp,
        (KeyModifiers::CONTROL, KeyCode::Char('s')) => ViewAction::StartExport,
        (KeyModifiers::NONE, KeyCode::Char(':')) => ViewAction::OpenFilter,
        _ => ViewAction::None,
    }
}
