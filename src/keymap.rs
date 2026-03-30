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
            (KeyModifiers::SHIFT, Char('?'), ToggleHelp),
            (KeyModifiers::CONTROL, Char('s'), StartExport),
            (KeyModifiers::NONE, Char(':'), OpenFilter),
        ];

        Self { bindings }
    }
}
