//! Shared raw-mode / alternate-screen lifecycle for the ratatui-based TUIs
//! (server status screen, chord navigator, slider).

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub(crate) type TuiTerminal = Terminal<CrosstermBackend<std::io::Stdout>>;

/// Put the terminal into raw mode + alternate screen with mouse capture and
/// return a cleared, cursor-hidden terminal.
pub(crate) fn enter() -> Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

/// Restore the terminal after [`enter`] (raw mode off, alternate screen left,
/// mouse capture disabled, cursor shown).
pub(crate) fn leave(terminal: &mut TuiTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Temporarily leave the TUI so external processes can use the terminal.
pub(crate) fn suspend(terminal: &mut TuiTerminal) -> Result<()> {
    leave(terminal)
}

/// Re-enter the TUI after [`suspend`], keeping the existing terminal/buffer.
pub(crate) fn resume(terminal: &mut TuiTerminal) -> Result<()> {
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.hide_cursor()?;
    Ok(())
}
