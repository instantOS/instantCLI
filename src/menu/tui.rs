//! Terminal User Interface for the Menu Server

use anyhow::Result;
use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::time::SystemTime;

use super::terminal::{self, TuiTerminal};

/// Terminal User Interface for the Menu Server
pub struct MenuServerTui {
    terminal: Option<TuiTerminal>,
    force_redraw: bool,
}

impl MenuServerTui {
    /// Create a new TUI instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            terminal: Some(terminal::enter()?),
            force_redraw: false,
        })
    }

    /// Request a full redraw on next draw
    pub fn request_redraw(&mut self) {
        self.force_redraw = true;
    }

    /// Draw the main server status screen
    pub fn draw_status_screen(
        &mut self,
        has_scratchpad: bool,
        requests_processed: u64,
        start_time: SystemTime,
    ) -> Result<()> {
        if let Some(ref mut terminal) = self.terminal {
            let force_redraw = self.force_redraw;

            // Reset the force redraw flag after using it
            if force_redraw {
                self.force_redraw = false;
            }

            // If force redraw is requested, clear the entire terminal first
            if force_redraw {
                terminal.clear()?;
            }

            terminal.draw(|f| {
                let size = f.area();

                // Create a centered layout for the main content
                let main_area = Layout::vertical([
                    Constraint::Length(3), // Title
                    Constraint::Length(1), // Spacer
                    Constraint::Length(3), // Main message
                    Constraint::Length(1), // Status
                ])
                .flex(Flex::Center)
                .split(size);

                // Create a centered area for the content block
                let content_width = Constraint::Percentage(60);
                let [content_area] = Layout::horizontal([content_width])
                    .flex(Flex::Center)
                    .areas(main_area[2]);

                // Title with styling - show mode in title
                let mode_text = if has_scratchpad {
                    "Menu Server"
                } else {
                    "Menu Server (No Scratchpad)"
                };
                let title = Paragraph::new(Line::from(vec![
                    Span::styled(
                        "InstantCLI",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {mode_text}"), Style::default().fg(Color::Gray)),
                ]))
                .alignment(Alignment::Center);

                // Main message with blue styling as specified
                let main_message = Paragraph::new("waiting for menu requests")
                    .style(
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    )
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Blue))
                            .title(" Status "),
                    )
                    .alignment(Alignment::Center);

                // Status info
                let status_text = format!(
                    "Requests: {} | Uptime: {}s",
                    requests_processed,
                    start_time.elapsed().unwrap_or_default().as_secs()
                );
                let status = Paragraph::new(status_text)
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center);

                // Instructions
                let instructions = Paragraph::new("Menu server running - input is passed to menus")
                    .style(
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::DIM),
                    )
                    .alignment(Alignment::Center);

                // Render everything
                f.render_widget(Clear, size); // Clear the entire screen
                f.render_widget(title, main_area[0]);
                f.render_widget(main_message, content_area);
                f.render_widget(status, main_area[3]);
                f.render_widget(
                    instructions,
                    Layout::vertical([Constraint::Length(1)])
                        .flex(Flex::End)
                        .split(size)[0],
                );
            })?;
        }
        Ok(())
    }

    /// Temporarily suspend TUI (for external process handling)
    pub fn suspend(&mut self) -> Result<()> {
        if let Some(ref mut terminal) = self.terminal {
            terminal::suspend(terminal)?;
        }
        Ok(())
    }

    /// Resume TUI after suspension
    pub fn resume(&mut self) -> Result<()> {
        if let Some(ref mut terminal) = self.terminal {
            terminal::resume(terminal)?;
        }
        // Request a full redraw after resume
        self.request_redraw();
        Ok(())
    }

    /// Clean up terminal
    pub fn cleanup(&mut self) -> Result<()> {
        if let Some(ref mut terminal) = self.terminal {
            terminal::leave(terminal)?;
        }
        Ok(())
    }
}

impl Drop for MenuServerTui {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
