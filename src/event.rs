use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEvent};

/// Events consumed by the application.
pub enum AppEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
    Tick,
}

/// Poll for the next event. Returns `Tick` if no event arrives within `tick_rate`.
pub fn poll(tick_rate: Duration) -> Result<AppEvent> {
    if event::poll(tick_rate)? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => Ok(AppEvent::Key(key)),
            Event::Mouse(mouse) => Ok(AppEvent::Mouse(mouse)),
            Event::Resize(_, _) => Ok(AppEvent::Resize),
            _ => Ok(AppEvent::Tick),
        }
    } else {
        Ok(AppEvent::Tick)
    }
}
