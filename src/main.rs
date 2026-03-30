mod app;
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

use crate::theme::Theme;

/// Theme selection for the UI.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

impl ThemeChoice {
    fn into_theme(self) -> Theme {
        match self {
            ThemeChoice::Dark => Theme::dark(),
            ThemeChoice::Light => Theme::light(),
        }
    }
}

#[derive(ClapParser, Debug)]
#[command(name = "jlens", about = "Ultra-performant terminal JSON viewer")]
struct Cli {
    /// Path to the JSON file to open, or "-" to read from stdin
    file: Option<PathBuf>,

    /// Color theme
    #[arg(long, default_value_t, value_enum)]
    theme: ThemeChoice,

    /// Compare FILE against DIFF_FILE and show a structural diff
    #[arg(long)]
    diff: Option<PathBuf>,

    /// Parse the file and print timing info without launching the TUI (benchmarking)
    #[arg(long, hide = true)]
    bench: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let theme = cli.theme.into_theme();

    // Bench mode: parse only, print timing, exit
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
        println!(
            "size:     {} bytes ({:.1} MB)",
            size,
            size as f64 / 1_048_576.0
        );
        println!("strategy: {}", strategy);
        println!("nodes:    {}", nodes);
        println!("parse:    {}ms", parse_ms);
        return Ok(());
    }

    // Diff mode: requires a file argument
    if let Some(diff_path) = cli.diff {
        match cli.file {
            Some(ref path) => {
                return app::run_diff(path, &diff_path, theme);
            }
            None => {
                eprintln!("\x1b[1;31merror\x1b[0m: --diff requires a FILE argument");
                eprintln!("Usage: jlens <file_a.json> --diff <file_b.json>");
                std::process::exit(1);
            }
        }
    }

    match cli.file {
        Some(path) if path.to_str() == Some("-") => app::run_stdin(theme),
        Some(path) => app::run_file(&path, theme),
        None => {
            if std::io::stdin().is_terminal() {
                Cli::parse_from(["jlens", "--help"]);
                Ok(())
            } else {
                app::run_stdin(theme)
            }
        }
    }
}
