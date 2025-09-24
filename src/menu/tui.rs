use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Flex, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::io::stdout;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::SystemTime;

/// Terminal User Interface for the Menu Server
pub struct MenuServerTui {
    terminal: Option<Terminal<CrosstermBackend<std::io::Stdout>>>,
    show_help: Arc<AtomicBool>,
    force_redraw: bool,
}

impl MenuServerTui {
    /// Create a new TUI instance
    pub fn new() -> Result<Self> {
        let terminal = {
            enable_raw_mode()?;
            let mut stdout = stdout();
            execute!(
                stdout,
                crossterm::terminal::EnterAlternateScreen,
                EnableMouseCapture
            )?;
            let backend = CrosstermBackend::new(stdout);
            Some(Terminal::new(backend)?)
        };

        Ok(Self {
            terminal,
            show_help: Arc::new(AtomicBool::new(false)),
            force_redraw: false,
        })
    }

    /// Get the show_help flag
    pub fn show_help(&self) -> Arc<AtomicBool> {
        self.show_help.clone()
    }

    /// Toggle help display - disabled since keyboard input is not handled
    pub fn toggle_help(&self) {
        // Help toggle is disabled since we don't handle keyboard input
        // Let the show_help flag remain false always
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
            let show_help = self.show_help.load(Ordering::SeqCst);
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
                    Span::styled(format!(" {}", mode_text), Style::default().fg(Color::Gray)),
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

                // Show help popup if requested
                if show_help {
                    Self::draw_help_popup_static(f, size);
                }
            })?;
        }
        Ok(())
    }

    /// Draw the help popup
    fn draw_help_popup_static(f: &mut ratatui::Frame, size: ratatui::layout::Rect) {
        let vertical_layout = Layout::vertical([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .flex(Flex::Center)
        .split(size);

        // Safely get the middle area (index 1)
        if vertical_layout.len() > 1 {
            let help_popup_area = vertical_layout[1];

            let horizontal_layout = Layout::horizontal([
                Constraint::Percentage(20),
                Constraint::Percentage(60),
                Constraint::Percentage(20),
            ])
            .flex(Flex::Center)
            .split(help_popup_area);

            // Safely get the middle area (index 1)
            if horizontal_layout.len() > 1 {
                let help_content_area = horizontal_layout[1];

                let help_text = vec![
                    Line::from("Help - Menu Server").style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::from(""),
                    Line::from("No keyboard input handling")
                        .style(Style::default().fg(Color::Yellow)),
                    Line::from("All input is passed to menus")
                        .style(Style::default().fg(Color::Yellow)),
                    Line::from(""),
                    Line::from("The server waits for menu requests and")
                        .style(Style::default().fg(Color::Gray)),
                    Line::from("processes them when received.")
                        .style(Style::default().fg(Color::Gray)),
                ];

                let help_popup = Paragraph::new(help_text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .title(" Help ")
                            .title_style(
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                    )
                    .style(Style::default().fg(Color::White))
                    .alignment(Alignment::Left);

                f.render_widget(Clear, help_popup_area);
                f.render_widget(help_popup, help_content_area);
            }
        }
    }

    /// Temporarily suspend TUI (for external process handling)
    pub fn suspend(&mut self) -> Result<()> {
        if let Some(ref mut terminal) = self.terminal {
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                crossterm::terminal::LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
        }
        Ok(())
    }

    /// Resume TUI after suspension
    pub fn resume(&mut self) -> Result<()> {
        if let Some(ref mut terminal) = self.terminal {
            enable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                crossterm::terminal::EnterAlternateScreen,
                EnableMouseCapture
            )?;
            terminal.hide_cursor()?;
        }
        // Request a full redraw after resume
        self.request_redraw();
        Ok(())
    }

    /// Clean up terminal
    pub fn cleanup(&mut self) -> Result<()> {
        if let Some(ref mut terminal) = self.terminal {
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                crossterm::terminal::LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
        }
        Ok(())
    }
}

impl Drop for MenuServerTui {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
