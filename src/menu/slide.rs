use std::io::stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph};

use crate::menu_utils::{SliderCommand, SliderConfig};

const POLL_TIMEOUT: Duration = Duration::from_millis(150);
const DIGIT_SEQUENCE: [char; 10] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'];

/// Launch the slider TUI and return the confirmed value or `None` if cancelled.
pub fn run_slider(config: SliderConfig) -> Result<Option<i64>> {
    let mut app = SliderApp::new(config)?;
    let result = app.run();
    app.cleanup()?;
    result
}

struct SliderApp {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    config: SliderConfig,
    last_slider_area: Option<Rect>,
    last_dispatched_value: i64,
    cleaned_up: bool,
}

impl SliderApp {
    fn new(config: SliderConfig) -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        terminal.hide_cursor()?;

        let initial_value = config.value;
        if let Some(command) = config.command.as_ref()
            && let Err(err) = command.spawn_with_value(initial_value)
        {
            eprintln!("Failed to execute slider command: {err}");
        }

        Ok(Self {
            terminal,
            config,
            last_slider_area: None,
            last_dispatched_value: initial_value,
            cleaned_up: false,
        })
    }

    fn run(&mut self) -> Result<Option<i64>> {
        loop {
            self.draw()?;

            if !event::poll(POLL_TIMEOUT)? {
                continue;
            }

            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    if key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('c'))
                    {
                        return Ok(None);
                    }

                    if key_event.modifiers.is_empty()
                        && matches!(key_event.code, KeyCode::Esc | KeyCode::Char('q'))
                    {
                        return Ok(None);
                    }

                    match key_event.code {
                        KeyCode::Enter => return Ok(Some(self.config.value)),
                        KeyCode::Char('h') | KeyCode::Left => {
                            self.bump_value(-self.config.step);
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            self.bump_value(self.config.step);
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            self.bump_value(-self.config.large_step);
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            self.bump_value(self.config.large_step);
                        }
                        KeyCode::Char(digit) if digit.is_ascii_digit() => {
                            self.snap_to_digit(digit);
                        }
                        KeyCode::Home => {
                            self.update_value(self.config.min);
                        }
                        KeyCode::End => {
                            self.update_value(self.config.max);
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse_event) => match mouse_event.kind {
                    MouseEventKind::Down(crossterm::event::MouseButton::Left)
                    | MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                        self.snap_from_position(mouse_event.column, mouse_event.row);
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {
                    // Force redraw next loop iteration
                }
                _ => {}
            }
        }
    }

    fn draw(&mut self) -> Result<()> {
        let value = self.config.value;
        let min = self.config.min;
        let max = self.config.max;
        let ratio = self.config.ratio();
        let label = self
            .config
            .label
            .clone()
            .unwrap_or_else(|| "Instant Slider".to_string());

        let display_label = format!("{label}: {value}");
        let range_label = format!("{min} – {max}");
        let help_text = Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(" accept  •  "),
            Span::styled("Esc/q", Style::default().fg(Color::Red)),
            Span::raw(" quit  •  "),
            Span::styled("h/l", Style::default().fg(Color::Cyan)),
            Span::raw(" ±step  •  "),
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(" ±big step  •  Digits jump (1 left … 0 max)"),
        ]);

        self.terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Clear, area);

            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Min(5),
                        Constraint::Length(2),
                    ]
                    .as_ref(),
                )
                .split(area);

            let title = Paragraph::new(Line::from(vec![
                Span::styled(
                    "instantCLI",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" — Slider"),
            ]))
            .alignment(Alignment::Center);

            let value_line = Paragraph::new(Line::from(vec![
                Span::styled(display_label.clone(), Style::default().fg(Color::Cyan)),
                Span::raw("  •  "),
                Span::styled(range_label.clone(), Style::default().fg(Color::Gray)),
            ]))
            .alignment(Alignment::Center);

            let slider_block = Block::default()
                .borders(Borders::ALL)
                .title(format!(" {label} "))
                .title_alignment(Alignment::Left);

            // Calculate label color based on whether it's over the gauge or background
            let label_text = format!(" {value} ");
            let label_color = if ratio > 0.5 {
                // When slider is more than halfway, use dark text (visible over green gauge)
                Color::Black
            } else {
                // When slider is less than halfway, use light text (visible over dark background)
                Color::White
            };

            let gauge = Gauge::default()
                .block(slider_block)
                .ratio(ratio.clamp(0.0, 1.0))
                .gauge_style(
                    Style::default()
                        .fg(Color::Green)
                        .bg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
                .label(Span::styled(label_text, Style::default().fg(label_color)));

            frame.render_widget(title, vertical[0]);
            frame.render_widget(value_line, vertical[1]);

            frame.render_widget(gauge, vertical[2]);
            self.last_slider_area = Some(vertical[2]);

            let help = Paragraph::new(help_text.clone())
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(help, vertical[3]);
        })?;

        Ok(())
    }

    fn bump_value(&mut self, delta: i64) {
        if self.config.apply_delta(delta) {
            self.dispatch_if_changed();
        }
    }

    fn update_value(&mut self, target: i64) {
        let target = self.config.clamp(target);
        if target != self.config.value {
            self.config.set_value(target);
            self.dispatch_if_changed();
        }
    }

    fn snap_to_digit(&mut self, digit: char) {
        if let Some(position) = DIGIT_SEQUENCE.iter().position(|&c| c == digit) {
            let fraction = position as f64 / (DIGIT_SEQUENCE.len() - 1) as f64;
            if self.config.snap_to_fraction(fraction) {
                self.dispatch_if_changed();
            }
        }
    }

    fn snap_from_position(&mut self, column: u16, row: u16) {
        let area = match self.last_slider_area {
            Some(area) if area.width > 1 && area.height > 0 => area,
            _ => return,
        };

        if row < area.y || row >= area.y.saturating_add(area.height) {
            return;
        }

        if column < area.x || column >= area.x + area.width {
            return;
        }

        let effective_width = area.width.saturating_sub(2).max(1);
        let relative_x = column.saturating_sub(area.x + 1).min(effective_width);
        let fraction = (relative_x as f64) / (effective_width as f64);

        if self.config.snap_to_fraction(fraction) {
            self.dispatch_if_changed();
        }
    }

    fn dispatch_if_changed(&mut self) {
        if self.config.value == self.last_dispatched_value {
            return;
        }

        if let Some(command) = self.config.command.as_ref()
            && let Err(err) = command.spawn_with_value(self.config.value)
        {
            eprintln!("Failed to execute slider command: {err}");
        }

        self.last_dispatched_value = self.config.value;
    }

    fn cleanup(&mut self) -> Result<()> {
        if self.cleaned_up {
            return Ok(());
        }

        disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        self.terminal.show_cursor()?;
        self.cleaned_up = true;
        Ok(())
    }
}

impl Drop for SliderApp {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Convenience helper to run the slider with arguments typically provided by CLI parsing.
pub fn run_slider_command(request: &super::protocol::SliderRequest) -> Result<Option<i64>> {
    let command = SliderCommand::from_argv(&request.command)?;
    let config = SliderConfig::new(
        request.min,
        request.max,
        request.value,
        request.step,
        request.big_step,
        request.label.clone(),
        command,
    )?;
    run_slider(config)
}
