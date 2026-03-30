mod app;
mod config;
mod diff;
mod event;
mod filter;
mod keymap;
mod model;
mod parser;
mod search;
mod theme;
mod ui;
pub(crate) mod util;
mod views;

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser as ClapParser;

use crate::config::Config;
use crate::theme::Theme;

#[derive(ClapParser, Debug)]
#[command(name = "jlens", about = "Ultra-performant terminal JSON viewer")]
struct Cli {
    /// Path to the JSON file to open, or "-" to read from stdin
    file: Option<PathBuf>,

    /// Color theme (overrides config)
    #[arg(long, value_enum)]
    theme: Option<ThemeArg>,

    /// Compare FILE against DIFF_FILE and show a structural diff
    #[arg(long)]
    diff: Option<PathBuf>,

    /// Path to config file (default: ~/.config/jlens/config.toml)
    #[arg(long)]
    config: Option<String>,

    /// Generate default config file and exit
    #[arg(long)]
    init: bool,

    /// Parse the file and print timing info without launching the TUI
    #[arg(long, hide = true)]
    bench: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum ThemeArg {
    Dark,
    Light,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // --init: generate config file and exit
    if cli.init {
        let path = cli
            .config
            .map(PathBuf::from)
            .or_else(Config::default_path);
        let Some(path) = path else {
            eprintln!("Could not determine config directory");
            std::process::exit(1);
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, Config::generate_default())?;
        println!("Config written to {}", path.display());
        return Ok(());
    }

    // Load config
    let cfg = Config::load(cli.config.as_deref());

    // Theme: CLI flag > config > default
    let theme = match cli.theme {
        Some(ThemeArg::Dark) => Theme::dark(),
        Some(ThemeArg::Light) => Theme::light(),
        None => Theme::from_config(cfg.theme.base.as_deref(), &cfg.theme.overrides),
    };

    // Keymap: defaults + config overrides
    let keymap = if cfg.keybindings.is_empty() {
        keymap::KeyMap::default()
    } else {
        keymap::KeyMap::from_config(&cfg.keybindings)
    };

    // Bench mode
    if cli.bench {
        let Some(ref path) = cli.file else {
            eprintln!("--bench requires a FILE argument");
            std::process::exit(1);
        };
        let start = std::time::Instant::now();
        let outcome = parser::parse_file_ex(path)?;
        let parse_ms = start.elapsed().as_millis();
        let (nodes, strategy) = match outcome {
            parser::ParseOutcome::Full(doc) => (doc.metadata().total_nodes, "full"),
            parser::ParseOutcome::Lazy(lazy) => {
                let doc = lazy.to_document();
                (doc.metadata().total_nodes, "lazy")
            }
        };
        let size = std::fs::metadata(path)?.len();
        println!("file:     {}", path.display());
        println!("size:     {} bytes ({:.1} MB)", size, size as f64 / 1_048_576.0);
        println!("strategy: {strategy}");
        println!("nodes:    {nodes}");
        println!("parse:    {parse_ms}ms");
        return Ok(());
    }

    // Diff mode
    if let Some(diff_path) = cli.diff {
        match cli.file {
            Some(ref path) => return app::run_diff(path, &diff_path, theme),
            None => {
                eprintln!("\x1b[1;31merror\x1b[0m: --diff requires a FILE argument");
                eprintln!("Usage: jlens <file_a.json> --diff <file_b.json>");
                std::process::exit(1);
            }
        }
    }

    // Normal mode
    match cli.file {
        Some(path) if path.to_str() == Some("-") => app::run_stdin_with(theme, keymap),
        Some(path) => app::run_file_with(&path, theme, keymap),
        None => {
            if std::io::stdin().is_terminal() {
                Cli::parse_from(["jlens", "--help"]);
                Ok(())
            } else {
                app::run_stdin_with(theme, keymap)
            }
        }
    }
}
