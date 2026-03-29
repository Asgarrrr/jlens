use ratatui::style::{Color, Modifier, Style};

/// Color theme for the JSON viewer.
#[derive(Debug, Clone)]
pub struct Theme {
    // General
    pub bg: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,

    // JSON syntax
    pub key: Style,
    pub string: Style,
    pub number: Style,
    pub boolean: Style,
    pub null: Style,
    pub bracket: Style,

    // UI chrome
    pub toolbar_bg: Color,
    pub toolbar_fg: Color,
    pub toolbar_active_bg: Color,
    pub toolbar_active_fg: Color,
    pub status_bg: Color,
    pub status_fg: Color,
    pub tree_guide: Color,

    // Search
    pub search_match: Style,
    #[allow(dead_code)]
    pub search_current: Style,

    // Flash message
    pub flash_bg: Color,
    pub flash_fg: Color,

    // Diff
    pub diff_added: Style,
    pub diff_removed: Style,
    pub diff_modified: Style,
}

impl Theme {
    /// Dark theme (Catppuccin Mocha) — default.
    pub fn dark() -> Self {
        Self {
            bg: Color::Rgb(22, 22, 30),
            fg: Color::Rgb(205, 214, 244),
            fg_dim: Color::Rgb(108, 112, 134),
            selection_bg: Color::Rgb(49, 50, 68),
            selection_fg: Color::Rgb(205, 214, 244),

            key: Style::new()
                .fg(Color::Rgb(137, 180, 250))
                .add_modifier(Modifier::BOLD),
            string: Style::new().fg(Color::Rgb(166, 227, 161)),
            number: Style::new().fg(Color::Rgb(250, 179, 135)),
            boolean: Style::new().fg(Color::Rgb(203, 166, 247)),
            null: Style::new().fg(Color::Rgb(108, 112, 134)),
            bracket: Style::new().fg(Color::Rgb(147, 153, 178)),

            toolbar_bg: Color::Rgb(30, 30, 46),
            toolbar_fg: Color::Rgb(147, 153, 178),
            toolbar_active_bg: Color::Rgb(137, 180, 250),
            toolbar_active_fg: Color::Rgb(30, 30, 46),
            status_bg: Color::Rgb(30, 30, 46),
            status_fg: Color::Rgb(147, 153, 178),
            tree_guide: Color::Rgb(88, 91, 112),

            search_match: Style::new()
                .bg(Color::Rgb(249, 226, 175))
                .fg(Color::Rgb(30, 30, 46)),
            search_current: Style::new()
                .bg(Color::Rgb(250, 179, 135))
                .fg(Color::Rgb(30, 30, 46))
                .add_modifier(Modifier::BOLD),

            flash_bg: Color::Rgb(166, 227, 161),
            flash_fg: Color::Rgb(30, 30, 46),

            // Catppuccin Mocha: green=A6E3A1, red=F38BA8, yellow=F9E2AF
            diff_added: Style::new().fg(Color::Rgb(166, 227, 161)),
            diff_removed: Style::new().fg(Color::Rgb(243, 139, 168)),
            diff_modified: Style::new().fg(Color::Rgb(249, 226, 175)),
        }
    }

    /// Light theme (Catppuccin Latte).
    pub fn light() -> Self {
        Self {
            bg: Color::Rgb(239, 241, 245),
            fg: Color::Rgb(76, 79, 105),
            fg_dim: Color::Rgb(140, 143, 161),
            selection_bg: Color::Rgb(188, 192, 204),
            selection_fg: Color::Rgb(76, 79, 105),

            key: Style::new()
                .fg(Color::Rgb(30, 102, 245))
                .add_modifier(Modifier::BOLD),
            string: Style::new().fg(Color::Rgb(64, 160, 43)),
            number: Style::new().fg(Color::Rgb(254, 100, 11)),
            boolean: Style::new().fg(Color::Rgb(136, 57, 239)),
            null: Style::new().fg(Color::Rgb(140, 143, 161)),
            bracket: Style::new().fg(Color::Rgb(108, 111, 133)),

            toolbar_bg: Color::Rgb(220, 224, 232),
            toolbar_fg: Color::Rgb(108, 111, 133),
            toolbar_active_bg: Color::Rgb(30, 102, 245),
            toolbar_active_fg: Color::Rgb(239, 241, 245),
            status_bg: Color::Rgb(220, 224, 232),
            status_fg: Color::Rgb(108, 111, 133),
            tree_guide: Color::Rgb(156, 160, 176),

            search_match: Style::new()
                .bg(Color::Rgb(223, 142, 29))
                .fg(Color::Rgb(239, 241, 245)),
            search_current: Style::new()
                .bg(Color::Rgb(254, 100, 11))
                .fg(Color::Rgb(239, 241, 245))
                .add_modifier(Modifier::BOLD),

            flash_bg: Color::Rgb(64, 160, 43),
            flash_fg: Color::Rgb(239, 241, 245),

            // Catppuccin Latte: green=40A02B, red=D20F39, yellow=DF8E1D
            diff_added: Style::new().fg(Color::Rgb(64, 160, 43)),
            diff_removed: Style::new().fg(Color::Rgb(210, 15, 57)),
            diff_modified: Style::new().fg(Color::Rgb(223, 142, 29)),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
