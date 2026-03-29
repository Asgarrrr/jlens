use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEvent};

/// Abstraction over crossterm's event polling.
/// Provides a clean interface for the app loop.
pub struct EventReader {
    tick_rate: Duration,
}

/// Events consumed by the application.
pub enum AppEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
    Tick,
}

impl EventReader {
    pub fn new(tick_rate: Duration) -> Self {
        Self { tick_rate }
    }

    /// Poll for the next event. Returns `Tick` if no event arrives within `tick_rate`.
    pub fn next(&self) -> Result<AppEvent> {
        if event::poll(self.tick_rate)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    Ok(AppEvent::Key(key))
                }
                Event::Key(_) => Ok(AppEvent::Tick),
                Event::Mouse(mouse) => Ok(AppEvent::Mouse(mouse)),
                Event::Resize(_, _) => Ok(AppEvent::Resize),
                _ => Ok(AppEvent::Tick),
            }
        } else {
            Ok(AppEvent::Tick)
        }
    }
}
