use ratatui::style::{Color, Modifier, Style};

/// Color theme for the JSON viewer.
///
/// Composite styles are pre-computed at construction time to avoid
/// rebuilding `Style` objects on every frame (~10 fps × 50+ calls).
#[derive(Debug, Clone)]
pub struct Theme {
    // General
    pub bg: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub selection_bg: Color,

    // JSON syntax
    pub key: Style,
    pub string: Style,
    pub number: Style,
    pub boolean: Style,
    pub null: Style,
    pub bracket: Style,

    // Pre-computed composite styles
    pub bg_style: Style,
    pub fg_style: Style,
    pub fg_dim_style: Style,
    pub fg_bold_style: Style,
    pub selection_style: Style,
    pub tree_guide_style: Style,

    // Toolbar
    pub toolbar_bg_style: Style,
    pub toolbar_brand_style: Style,
    pub toolbar_active_style: Style,
    pub toolbar_inactive_style: Style,

    // Status bar
    pub status_style: Style,
    pub status_fg_style: Style,
    pub status_dim_style: Style,

    // Flash
    pub flash_style: Style,

    // Search
    pub search_match: Style,
    pub search_current: Style,

    // Diff
    pub diff_added: Style,
    pub diff_removed: Style,
    pub diff_modified: Style,
    pub diff_added_bg: Style,
    pub diff_removed_bg: Style,
    pub diff_modified_bg: Style,

    // Semantic
    pub error_style: Style,
    pub warning_style: Style,
    pub success_style: Style,

    // Selection
    pub selected_indicator_style: Style,
    pub alt_row_bg: Style,

    // Tree
    pub tree_icon_style: Style,

    // Scrollbar
    pub scrollbar_thumb_style: Style,
    pub scrollbar_track_style: Style,

    // Help overlay
    pub help_border_style: Style,
    pub help_title_style: Style,
}

impl Theme {
    /// Dark theme (Catppuccin Mocha) — default.
    pub fn dark() -> Self {
        let bg = Color::Rgb(22, 22, 30);
        let fg = Color::Rgb(205, 214, 244);
        let fg_dim = Color::Rgb(108, 112, 134);
        let selection_bg = Color::Rgb(49, 50, 68);
        let toolbar_bg = Color::Rgb(30, 30, 46);
        let toolbar_fg = Color::Rgb(147, 153, 178);
        let toolbar_active_bg = Color::Rgb(137, 180, 250);
        let toolbar_active_fg = Color::Rgb(30, 30, 46);
        let status_bg = Color::Rgb(30, 30, 46);
        let status_fg = Color::Rgb(147, 153, 178);
        let tree_guide = Color::Rgb(88, 91, 112);
        let flash_bg = Color::Rgb(166, 227, 161);
        let flash_fg = Color::Rgb(30, 30, 46);

        Self {
            bg, fg, fg_dim, selection_bg,

            key: Style::new().fg(Color::Rgb(137, 180, 250)).add_modifier(Modifier::BOLD),
            string: Style::new().fg(Color::Rgb(166, 227, 161)),
            number: Style::new().fg(Color::Rgb(250, 179, 135)),
            boolean: Style::new().fg(Color::Rgb(203, 166, 247)),
            null: Style::new().fg(fg_dim),
            bracket: Style::new().fg(toolbar_fg),

            bg_style: Style::new().bg(bg),
            fg_style: Style::new().fg(fg),
            fg_dim_style: Style::new().fg(fg_dim),
            fg_bold_style: Style::new().fg(fg).add_modifier(Modifier::BOLD),
            selection_style: Style::new().fg(fg).bg(selection_bg),
            tree_guide_style: Style::new().fg(tree_guide),

            toolbar_bg_style: Style::new().bg(toolbar_bg),
            toolbar_brand_style: Style::new().fg(toolbar_active_fg).bg(toolbar_active_bg).add_modifier(Modifier::BOLD),
            toolbar_active_style: Style::new().fg(toolbar_active_fg).bg(toolbar_active_bg),
            toolbar_inactive_style: Style::new().fg(toolbar_fg).bg(toolbar_bg),

            status_style: Style::new().bg(status_bg),
            status_fg_style: Style::new().fg(status_fg).bg(status_bg),
            status_dim_style: Style::new().fg(fg_dim).bg(status_bg),

            flash_style: Style::new().fg(flash_fg).bg(flash_bg),

            search_match: Style::new().bg(Color::Rgb(249, 226, 175)).fg(Color::Rgb(30, 30, 46)),
            search_current: Style::new().bg(Color::Rgb(250, 179, 135)).fg(Color::Rgb(30, 30, 46)).add_modifier(Modifier::BOLD),

            diff_added: Style::new().fg(Color::Rgb(166, 227, 161)),
            diff_removed: Style::new().fg(Color::Rgb(243, 139, 168)),
            diff_modified: Style::new().fg(Color::Rgb(249, 226, 175)),
            diff_added_bg: Style::new().bg(Color::Rgb(22, 38, 28)),
            diff_removed_bg: Style::new().bg(Color::Rgb(42, 22, 28)),
            diff_modified_bg: Style::new().bg(Color::Rgb(42, 38, 22)),

            error_style: Style::new().fg(Color::Rgb(243, 139, 168)),
            warning_style: Style::new().fg(Color::Rgb(249, 226, 175)),
            success_style: Style::new().fg(Color::Rgb(166, 227, 161)),

            selected_indicator_style: Style::new().fg(toolbar_active_bg),
            alt_row_bg: Style::new().bg(Color::Rgb(26, 26, 36)),
            tree_icon_style: Style::new().fg(toolbar_active_bg),

            scrollbar_thumb_style: Style::new().fg(fg_dim),
            scrollbar_track_style: Style::new().fg(tree_guide),
            help_border_style: Style::new().fg(tree_guide),
            help_title_style: Style::new().fg(toolbar_active_bg).add_modifier(Modifier::BOLD),
        }
    }

    /// Light theme (Catppuccin Latte).
    pub fn light() -> Self {
        let bg = Color::Rgb(239, 241, 245);
        let fg = Color::Rgb(76, 79, 105);
        let fg_dim = Color::Rgb(140, 143, 161);
        let selection_bg = Color::Rgb(188, 192, 204);
        let toolbar_bg = Color::Rgb(220, 224, 232);
        let toolbar_fg = Color::Rgb(108, 111, 133);
        let toolbar_active_bg = Color::Rgb(30, 102, 245);
        let toolbar_active_fg = Color::Rgb(239, 241, 245);
        let status_bg = Color::Rgb(220, 224, 232);
        let status_fg = Color::Rgb(108, 111, 133);
        let tree_guide = Color::Rgb(156, 160, 176);
        let flash_bg = Color::Rgb(64, 160, 43);
        let flash_fg = Color::Rgb(239, 241, 245);

        Self {
            bg, fg, fg_dim, selection_bg,

            key: Style::new().fg(Color::Rgb(30, 102, 245)).add_modifier(Modifier::BOLD),
            string: Style::new().fg(Color::Rgb(64, 160, 43)),
            number: Style::new().fg(Color::Rgb(254, 100, 11)),
            boolean: Style::new().fg(Color::Rgb(136, 57, 239)),
            null: Style::new().fg(fg_dim),
            bracket: Style::new().fg(toolbar_fg),

            bg_style: Style::new().bg(bg),
            fg_style: Style::new().fg(fg),
            fg_dim_style: Style::new().fg(fg_dim),
            fg_bold_style: Style::new().fg(fg).add_modifier(Modifier::BOLD),
            selection_style: Style::new().fg(fg).bg(selection_bg),
            tree_guide_style: Style::new().fg(tree_guide),

            toolbar_bg_style: Style::new().bg(toolbar_bg),
            toolbar_brand_style: Style::new().fg(toolbar_active_fg).bg(toolbar_active_bg).add_modifier(Modifier::BOLD),
            toolbar_active_style: Style::new().fg(toolbar_active_fg).bg(toolbar_active_bg),
            toolbar_inactive_style: Style::new().fg(toolbar_fg).bg(toolbar_bg),

            status_style: Style::new().bg(status_bg),
            status_fg_style: Style::new().fg(status_fg).bg(status_bg),
            status_dim_style: Style::new().fg(fg_dim).bg(status_bg),

            flash_style: Style::new().fg(flash_fg).bg(flash_bg),

            search_match: Style::new().bg(Color::Rgb(223, 142, 29)).fg(Color::Rgb(239, 241, 245)),
            search_current: Style::new().bg(Color::Rgb(254, 100, 11)).fg(Color::Rgb(239, 241, 245)).add_modifier(Modifier::BOLD),

            diff_added: Style::new().fg(Color::Rgb(64, 160, 43)),
            diff_removed: Style::new().fg(Color::Rgb(210, 15, 57)),
            diff_modified: Style::new().fg(Color::Rgb(223, 142, 29)),
            diff_added_bg: Style::new().bg(Color::Rgb(226, 243, 222)),
            diff_removed_bg: Style::new().bg(Color::Rgb(248, 218, 224)),
            diff_modified_bg: Style::new().bg(Color::Rgb(248, 238, 212)),

            error_style: Style::new().fg(Color::Rgb(210, 15, 57)),
            warning_style: Style::new().fg(Color::Rgb(223, 142, 29)),
            success_style: Style::new().fg(Color::Rgb(64, 160, 43)),

            selected_indicator_style: Style::new().fg(toolbar_active_bg),
            alt_row_bg: Style::new().bg(Color::Rgb(230, 233, 239)),
            tree_icon_style: Style::new().fg(toolbar_active_bg),

            scrollbar_thumb_style: Style::new().fg(fg_dim),
            scrollbar_track_style: Style::new().fg(tree_guide),
            help_border_style: Style::new().fg(tree_guide),
            help_title_style: Style::new().fg(toolbar_active_bg).add_modifier(Modifier::BOLD),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
