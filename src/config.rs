use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

/// User configuration loaded from TOML.
/// All fields are optional — missing values use built-in defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub search: SearchConfig,
    pub keybindings: HashMap<String, String>,
    pub theme: ThemeConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub default_view: String,
    pub tick_rate_ms: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_view: "tree".into(),
            tick_rate_ms: 33,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub case_sensitive: bool,
    pub regex: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub base: Option<String>,
    pub overrides: HashMap<String, String>,
}

impl Config {
    /// Load config from the default path, or return defaults if no file exists.
    pub fn load(custom_path: Option<&str>) -> Self {
        let path = custom_path
            .map(PathBuf::from)
            .or_else(Self::default_path);

        let Some(path) = path else {
            return Self::default();
        };

        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!(
                        "\x1b[1;33mwarning\x1b[0m: invalid config at {}: {}",
                        path.display(),
                        e
                    );
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// XDG-compliant config path.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("jlens").join("config.toml"))
    }

    /// Generate a commented default config file.
    pub fn generate_default() -> String {
        r##"# jlens configuration
# Place this file at:
#   Linux:  ~/.config/jlens/config.toml
#   macOS:  ~/Library/Application Support/jlens/config.toml

[general]
# default_view = "tree"  # tree, table, raw, paths, stats
# tick_rate_ms = 33       # render interval (30fps)

[search]
# case_sensitive = false
# regex = false

[keybindings]
# Format: action = "key"
# Modifiers: ctrl+, shift+
# Examples:
# quit = "q"
# quit = "ctrl+c"
# search = "/"
# search = "ctrl+f"
# help = "?"
# filter = ":"
# export = "ctrl+s"
# move_up = "k"
# move_down = "j"
# page_up = "ctrl+u"
# page_down = "ctrl+d"
# home = "Home"
# end = "G"
# toggle_expand = "Enter"
# expand_node = "l"
# collapse_node = "h"
# expand_all = "e"
# collapse_all = "E"
# copy_value = "y"
# copy_path = "Y"
# next_column = "Tab"
# prev_column = "shift+Tab"
# cycle_sort = "s"
# switch_view_1 = "1"
# switch_view_2 = "2"
# switch_view_3 = "3"
# switch_view_4 = "4"
# switch_view_5 = "5"
# next_search_hit = "n"
# prev_search_hit = "N"

[theme]
# base = "dark"  # dark or light

[theme.overrides]
# Any color field from the theme can be overridden.
# Use hex colors: "#rrggbb"
# bg = "#1e1e2e"
# fg = "#cdd6f4"
# fg_dim = "#6c7086"
# selection_bg = "#313244"
# key = "#89b4fa"
# string = "#a6e3a1"
# number = "#fab387"
# boolean = "#cba6f7"
# null = "#6c7086"
# bracket = "#9399b2"
"##
        .to_string()
    }
}
