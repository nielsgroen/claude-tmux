mod app;
mod completion;
mod detection;
mod git;
mod input;
mod scroll_state;
mod session;
mod tmux;
mod ui;

use std::io::{self, stdout};

use anyhow::Result;
use crossterm::{
    event::{self, Event},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;

use crate::app::App;

/// Parse `--preview-percent <N>` (or `--preview-percent=<N>`) from the CLI.
/// Controls the preview pane height as a percentage of available space.
/// Defaults to 50; unparseable/out-of-range values clamp to 10..=100.
fn preview_percent_from_args() -> u16 {
    const DEFAULT: u16 = 50;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = if let Some(v) = arg.strip_prefix("--preview-percent=") {
            Some(v.to_string())
        } else if arg == "--preview-percent" {
            args.next()
        } else {
            None
        };
        if let Some(v) = value {
            return v.parse::<u16>().unwrap_or(DEFAULT).clamp(10, 100);
        }
    }
    DEFAULT
}

fn main() -> Result<()> {
    // Set up terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    // Run the app
    let result = run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new(preview_percent_from_args())?;

    loop {
        // Draw the UI
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        // Check if we should quit
        if app.should_quit {
            break;
        }

        // Handle events
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                input::handle_key(&mut app, key);
            }
        }

        // Refresh Claude status via content-change detection (self-throttled to 500 ms)
        app.tick_status();
    }

    Ok(())
}
