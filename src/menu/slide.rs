use std::io::stdout;
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use clap::ValueEnum;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SliderPreset {
    #[value(alias = "volume")]
    Audio,
    #[value(alias = "brightness")]
    #[value(alias = "bright")]
    Brightness,
}

pub(crate) struct PresetConfig {
    pub(crate) min: i64,
    pub(crate) max: i64,
    pub(crate) value: Option<i64>,
    pub(crate) step: Option<i64>,
    pub(crate) big_step: Option<i64>,
    pub(crate) label: Option<String>,
    pub(crate) command: Vec<String>,
}

impl SliderPreset {
    pub(crate) fn config(self) -> PresetConfig {
        match self {
            SliderPreset::Audio => PresetConfig {
                min: 0,
                max: 100,
                value: Self::wpctl_volume(),
                step: Some(1),
                big_step: Some(5),
                label: Some("Audio Volume".to_string()),
                command: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    Self::audio_command_script(),
                    "ins-menu-slide-audio".to_string(),
                ],
            },
            SliderPreset::Brightness => PresetConfig {
                min: 0,
                max: 100,
                value: Self::brightnessctl_percentage(),
                step: Some(1),
                big_step: Some(5),
                label: Some("Screen Brightness".to_string()),
                command: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    Self::brightness_command_script(),
                    "ins-menu-slide-brightness".to_string(),
                ],
            },
        }
    }

    fn audio_command_script() -> String {
        let mut script = String::from("value=\"$1\"\n\n");
        script.push_str("wpctl set-volume @DEFAULT_AUDIO_SINK@ \"${value}%\" 2>/dev/null\n\n");
        script.push_str(&Self::notification_script(
            "instantcli-volume",
            "audio-volume-medium-symbolic",
            "Volume [${value}%]",
        ));
        script
    }

    fn brightness_command_script() -> String {
        let mut script = String::from("value=\"$1\"\n\n");
        script.push_str("brightnessctl --quiet set \"${value}%\" 2>/dev/null\n\n");
        script.push_str(&Self::notification_script(
            "instantcli-brightness",
            "display-brightness-medium-symbolic",
            "Brightness [${value}%]",
        ));
        script
    }

    fn notification_script(stack_tag: &str, icon: &str, label: &str) -> String {
        format!(
            "dunstify --appname instantCLI \\\n    -h string:x-dunst-stack-tag:{stack_tag} \\\n    -h int:value:\"${{value}}\" \\\n    -i {icon} \\\n    \"{label}\" 2>/dev/null",
            stack_tag = stack_tag,
            icon = icon,
            label = label
        )
    }

    fn wpctl_volume() -> Option<i64> {
        let output = Self::command_output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
        let fraction = output.split_whitespace().find_map(|token| {
            let sanitized = token.trim_matches(|c: char| matches!(c, '[' | ']' | ',' | ':'));
            sanitized.parse::<f64>().ok()
        })?;

        let percent = (fraction * 100.0).trunc().clamp(0.0, 100.0);
        Some(percent as i64)
    }
    fn brightnessctl_percentage() -> Option<i64> {
        let current = Self::command_output("brightnessctl", &["get"])?
            .parse::<f64>()
            .ok()?;
        let max = Self::command_output("brightnessctl", &["max"])?
            .parse::<f64>()
            .ok()?;

        if max <= 0.0 {
            return None;
        }

        let percent = (current / max * 100.0).round().clamp(0.0, 100.0);
        Some(percent as i64)
    }

    fn command_output(program: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(program).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            None
        } else {
            Some(stdout)
        }
    }
}

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
                        && matches!(
                            key_event.code,
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
                        )
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
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollRight => {
                        self.bump_value(self.config.step);
                    }
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollLeft => {
                        self.bump_value(-self.config.step);
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
        let title = Self::build_title();
        let value_line = self.build_value_line();
        let gauge = self.build_gauge();
        let help = Self::build_help();

        let mut slider_area = None;
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

            frame.render_widget(title, vertical[0]);
            frame.render_widget(value_line, vertical[1]);
            frame.render_widget(gauge, vertical[2]);
            slider_area = Some(vertical[2]);
            frame.render_widget(help, vertical[3]);
        })?;
        self.last_slider_area = slider_area;

        Ok(())
    }

    fn build_title() -> Paragraph<'static> {
        Paragraph::new(Line::from(vec![
            Span::styled(
                "instantCLI",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" — Slider"),
        ]))
        .alignment(Alignment::Center)
    }

    fn build_value_line(&self) -> Paragraph<'static> {
        let label = self.config.label.as_deref().unwrap_or("Instant Slider");
        let display_label = format!("{label}: {}", self.config.value);
        let range_label = format!("{} – {}", self.config.min, self.config.max);

        Paragraph::new(Line::from(vec![
            Span::styled(display_label, Style::default().fg(Color::Cyan)),
            Span::raw("  •  "),
            Span::styled(range_label, Style::default().fg(Color::Gray)),
        ]))
        .alignment(Alignment::Center)
    }

    fn build_gauge(&self) -> Gauge<'static> {
        let ratio = self.config.ratio();
        let value = self.config.value;

        let slider_block = Block::default().borders(Borders::ALL);

        let label_text = format!(" {value} ");
        let label_color = if ratio > 0.5 {
            Color::Black
        } else {
            Color::White
        };

        Gauge::default()
            .block(slider_block)
            .ratio(ratio.clamp(0.0, 1.0))
            .gauge_style(
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .label(Span::styled(label_text, Style::default().fg(label_color)))
    }

    fn build_help() -> Paragraph<'static> {
        let help_text = Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(" accept  •  "),
            Span::styled("Esc/q/Q", Style::default().fg(Color::Red)),
            Span::raw(" quit  •  "),
            Span::styled("h/l", Style::default().fg(Color::Cyan)),
            Span::raw(" ±step  •  "),
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(" ±big step  •  Wheel ±step  •  Digits jump (1 left … 0 max)"),
        ]);

        Paragraph::new(help_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray))
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
    let config = SliderConfig::builder()
        .min(request.min)
        .max(request.max)
        .value(request.value)
        .step(request.step)
        .large_step(request.big_step)
        .label(request.label.clone())
        .command(command)
        .build()?;
    run_slider(config)
}
