use std::io::{self, stdout};
use std::panic::{AssertUnwindSafe, UnwindSafe};

use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// Set up the terminal (raw mode, alternate screen, mouse capture),
/// run the provided closure, and restore the terminal on exit or panic.
pub(crate) fn with_terminal<F>(f: F) -> Result<()>
where
    F: FnOnce(&mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> + UnwindSafe,
{
    enable_raw_mode()?;

    if let Err(e) = stdout()
        .execute(EnterAlternateScreen)
        .and_then(|s| s.execute(EnableMouseCapture))
    {
        let _ = disable_raw_mode();
        return Err(e.into());
    }

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend).expect("failed to create terminal");
        terminal.clear().expect("failed to clear terminal");
        f(&mut terminal)
    }));

    let _ = stdout().execute(DisableMouseCapture);
    let _ = disable_raw_mode();
    let _ = stdout().execute(LeaveAlternateScreen);

    match result {
        Ok(inner) => inner,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
