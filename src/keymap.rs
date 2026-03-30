use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Semantic actions produced by the keymap layer.
///
/// Views and the app dispatch on these instead of raw `KeyEvent`.
/// Modal handlers (search bar, export bar, filter bar) bypass this
/// layer since they handle text input directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    // Navigation (shared across views)
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Home,
    End,

    // Tree / Diff navigation
    ToggleExpand,
    ExpandNode,
    CollapseNode,
    ExpandAll,
    CollapseAll,

    // Table navigation
    NextColumn,
    PrevColumn,
    CycleSort,

    // Clipboard
    CopyValue,
    CopyPath,

    // Zoom
    ZoomIn,
    ZoomOut,

    // Global
    Quit,
    SwitchView(u8), // 1-5
    StartSearch,
    NextSearchHit,
    PrevSearchHit,
    ToggleHelp,
    StartExport,
    OpenFilter,
}

/// Maps raw key events to semantic actions.
///
/// The lookup is a simple linear scan over a sorted binding list.
/// With ~40 bindings this is faster than HashMap (no hashing overhead)
/// and trivially serializable for future config support.
pub(crate) struct KeyMap {
    bindings: Vec<(KeyModifiers, KeyCode, Action)>,
}

impl KeyMap {
    /// Build a keymap from config overrides applied on top of defaults.
    pub fn from_config(overrides: &HashMap<String, String>) -> Self {
        let mut keymap = Self::default();
        for (action_name, key_str) in overrides {
            let Some(action) = parse_action(action_name) else {
                eprintln!("\x1b[1;33mwarning\x1b[0m: unknown action '{action_name}' in keybindings");
                continue;
            };
            let Some((mods, code)) = parse_key(key_str) else {
                eprintln!("\x1b[1;33mwarning\x1b[0m: invalid key '{key_str}' for action '{action_name}'");
                continue;
            };
            // Remove existing bindings for this action, then add the new one.
            keymap.bindings.retain(|&(_, _, a)| a != action);
            keymap.bindings.push((mods, code, action));
        }
        keymap
    }

    /// Look up the action for a key event. Returns `None` if unmapped.
    pub fn resolve(&self, key: &KeyEvent) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(mods, code, _)| key.modifiers == *mods && key.code == *code)
            .map(|&(_, _, action)| action)
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        use Action::*;
        use KeyCode::*;

        let bindings = vec![
            // Navigation
            (KeyModifiers::NONE, Up, MoveUp),
            (KeyModifiers::NONE, Char('k'), MoveUp),
            (KeyModifiers::NONE, Down, MoveDown),
            (KeyModifiers::NONE, Char('j'), MoveDown),
            (KeyModifiers::CONTROL, Char('u'), Action::PageUp),
            (KeyModifiers::NONE, KeyCode::PageUp, Action::PageUp),
            (KeyModifiers::CONTROL, Char('d'), Action::PageDown),
            (KeyModifiers::NONE, KeyCode::PageDown, Action::PageDown),
            (KeyModifiers::NONE, KeyCode::Home, Action::Home),
            (KeyModifiers::NONE, KeyCode::End, Action::End),
            (KeyModifiers::SHIFT, Char('G'), Action::End),
            // Tree / Diff
            (KeyModifiers::NONE, Enter, ToggleExpand),
            (KeyModifiers::NONE, Char(' '), ToggleExpand),
            (KeyModifiers::NONE, Right, ExpandNode),
            (KeyModifiers::NONE, Char('l'), ExpandNode),
            (KeyModifiers::NONE, Left, CollapseNode),
            (KeyModifiers::NONE, Char('h'), CollapseNode),
            (KeyModifiers::NONE, Char('e'), ExpandAll),
            (KeyModifiers::SHIFT, Char('E'), CollapseAll),
            // Table
            (KeyModifiers::NONE, Tab, NextColumn),
            (KeyModifiers::SHIFT, BackTab, PrevColumn),
            (KeyModifiers::NONE, Char('s'), CycleSort),
            // Clipboard
            (KeyModifiers::NONE, Char('y'), CopyValue),
            (KeyModifiers::SHIFT, Char('Y'), CopyPath),
            // Zoom
            (KeyModifiers::NONE, Char('z'), ZoomIn),
            (KeyModifiers::SHIFT, Char('Z'), ZoomOut),
            // Global
            (KeyModifiers::NONE, Char('q'), Quit),
            (KeyModifiers::CONTROL, Char('c'), Quit),
            (KeyModifiers::NONE, Char('1'), SwitchView(1)),
            (KeyModifiers::NONE, Char('2'), SwitchView(2)),
            (KeyModifiers::NONE, Char('3'), SwitchView(3)),
            (KeyModifiers::NONE, Char('4'), SwitchView(4)),
            (KeyModifiers::NONE, Char('5'), SwitchView(5)),
            (KeyModifiers::NONE, Char('/'), StartSearch),
            (KeyModifiers::CONTROL, Char('f'), StartSearch),
            (KeyModifiers::NONE, Char('n'), NextSearchHit),
            (KeyModifiers::SHIFT, Char('N'), PrevSearchHit),
            (KeyModifiers::NONE, Char('?'), ToggleHelp),
            (KeyModifiers::SHIFT, Char('?'), ToggleHelp),
            (KeyModifiers::CONTROL, Char('s'), StartExport),
            (KeyModifiers::NONE, Char(':'), OpenFilter),
        ];

        Self { bindings }
    }
}

fn parse_action(name: &str) -> Option<Action> {
    Some(match name {
        "move_up" => Action::MoveUp,
        "move_down" => Action::MoveDown,
        "page_up" => Action::PageUp,
        "page_down" => Action::PageDown,
        "home" => Action::Home,
        "end" => Action::End,
        "toggle_expand" => Action::ToggleExpand,
        "expand_node" => Action::ExpandNode,
        "collapse_node" => Action::CollapseNode,
        "expand_all" => Action::ExpandAll,
        "collapse_all" => Action::CollapseAll,
        "next_column" => Action::NextColumn,
        "prev_column" => Action::PrevColumn,
        "cycle_sort" => Action::CycleSort,
        "copy_value" => Action::CopyValue,
        "copy_path" => Action::CopyPath,
        "zoom_in" => Action::ZoomIn,
        "zoom_out" => Action::ZoomOut,
        "quit" => Action::Quit,
        "search" => Action::StartSearch,
        "next_search_hit" => Action::NextSearchHit,
        "prev_search_hit" => Action::PrevSearchHit,
        "help" => Action::ToggleHelp,
        "export" => Action::StartExport,
        "filter" => Action::OpenFilter,
        "switch_view_1" => Action::SwitchView(1),
        "switch_view_2" => Action::SwitchView(2),
        "switch_view_3" => Action::SwitchView(3),
        "switch_view_4" => Action::SwitchView(4),
        "switch_view_5" => Action::SwitchView(5),
        _ => return None,
    })
}

fn parse_key(s: &str) -> Option<(KeyModifiers, KeyCode)> {
    let s = s.trim();
    let mut mods = KeyModifiers::NONE;
    let mut remaining = s;

    // Parse modifier prefixes: ctrl+, shift+
    loop {
        if let Some(rest) = remaining.strip_prefix("ctrl+") {
            mods |= KeyModifiers::CONTROL;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("shift+") {
            mods |= KeyModifiers::SHIFT;
            remaining = rest;
        } else {
            break;
        }
    }

    let code = match remaining {
        "Enter" | "enter" | "Return" => KeyCode::Enter,
        "Esc" | "esc" | "Escape" => KeyCode::Esc,
        "Tab" | "tab" => KeyCode::Tab,
        "BackTab" | "backtab" => KeyCode::BackTab,
        "Backspace" | "backspace" => KeyCode::Backspace,
        "Up" | "up" => KeyCode::Up,
        "Down" | "down" => KeyCode::Down,
        "Left" | "left" => KeyCode::Left,
        "Right" | "right" => KeyCode::Right,
        "Home" | "home" => KeyCode::Home,
        "End" | "end" => KeyCode::End,
        "PageUp" | "pageup" => KeyCode::PageUp,
        "PageDown" | "pagedown" => KeyCode::PageDown,
        "Space" | "space" | " " => KeyCode::Char(' '),
        c if c.len() == 1 => {
            let ch = c.chars().next()?;
            if ch.is_uppercase() {
                mods |= KeyModifiers::SHIFT;
                KeyCode::Char(ch)
            } else {
                KeyCode::Char(ch)
            }
        }
        _ => return None,
    };

    Some((mods, code))
}
