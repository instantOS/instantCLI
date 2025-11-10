use std::io::stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::menu_utils::{KeyChord, KeyChordAction, KeyChordChild, KeyChordNode};

const POLL_TIMEOUT: Duration = Duration::from_millis(200);

pub fn run_chord_demo() -> Result<i32> {
    let root = demo_tree();
    let mut demo = KeyChordNavigator::new(root)?;
    let action = demo.run()?;
    if let Some(action_id) = action {
        println!("{action_id}");
        Ok(0)
    } else {
        Ok(1)
    }
}

struct KeyChordNavigator {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    history: Vec<(KeyChordNode, Vec<String>)>,
    current_node: KeyChordNode,
    path: Vec<String>,
    cleaned_up: bool,
}

impl KeyChordNavigator {
    fn new(root: KeyChordNode) -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        terminal.hide_cursor()?;

        Ok(Self {
            terminal,
            history: Vec::new(),
            current_node: root,
            path: Vec::new(),
            cleaned_up: false,
        })
    }

    fn run(&mut self) -> Result<Option<String>> {
        let mut needs_redraw = true;

        loop {
            if needs_redraw {
                self.draw()?;
                needs_redraw = false;
            }

            if !event::poll(POLL_TIMEOUT)? {
                continue;
            }

            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    if key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('c'))
                    {
                        break;
                    }

                    match key_event.code {
                        KeyCode::Esc | KeyCode::Backspace => {
                            if let Some((node, path)) = self.history.pop() {
                                self.current_node = node;
                                self.path = path;
                                needs_redraw = true;
                            } else {
                                break;
                            }
                        }
                        KeyCode::Char('q') if key_event.modifiers.is_empty() => {
                            break;
                        }
                        code => {
                            if key_event.modifiers.is_empty() {
                                if let Some(chord) = self.current_node.find_chord(&code).cloned() {
                                    let label = key_label(&chord.key);
                                    match chord.child {
                                        KeyChordChild::Leaf(action) => {
                                            let action_id = action.id;
                                            self.cleanup()?;
                                            return Ok(Some(action_id));
                                        }
                                        KeyChordChild::Node(node) => {
                                            let breadcrumb =
                                                format!("{} ({})", label, chord.description);
                                            self.history.push((
                                                self.current_node.clone(),
                                                self.path.clone(),
                                            ));
                                            self.current_node = node;
                                            self.path.push(breadcrumb);
                                            needs_redraw = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Event::Resize(_, _) => needs_redraw = true,
                _ => {}
            }
        }

        self.cleanup()?;
        Ok(None)
    }

    fn draw(&mut self) -> Result<()> {
        let path_display = if self.path.is_empty() {
            "<root>".to_string()
        } else {
            self.path.join("  ›  ")
        };

        let node_description = self.current_node.description.clone();
        let items: Vec<ListItem> = self
            .current_node
            .chords()
            .iter()
            .map(|chord| match &chord.child {
                KeyChordChild::Node(node) => {
                    let line = Line::from(vec![
                        Span::styled(
                            format!(" {:>4} ", key_label(&chord.key)),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  "),
                        Span::raw(&chord.description),
                        Span::styled(
                            format!("  [{}]", node.description),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]);
                    ListItem::new(line)
                }
                KeyChordChild::Leaf(_) => {
                    let line = Line::from(vec![
                        Span::styled(
                            format!(" {:>4} ", key_label(&chord.key)),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  "),
                        Span::raw(&chord.description),
                    ]);
                    ListItem::new(line)
                }
            })
            .collect();

        let instructions = Line::from(vec![
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw("/"),
            Span::styled("Backspace", Style::default().fg(Color::Cyan)),
            Span::raw(" to go back  •  "),
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::raw(" to quit"),
        ]);

        self.terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Clear, area);

            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Length(2),
                        Constraint::Min(5),
                        Constraint::Length(1),
                    ]
                    .as_ref(),
                )
                .split(area);

            let title = Paragraph::new(Line::from(vec![
                Span::styled(
                    "KeyChord Demo",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" — "),
                Span::raw(node_description.clone()),
            ]));

            let path = Paragraph::new(Line::from(vec![
                Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
                Span::raw(path_display.clone()),
            ]));

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Available Chords "),
                )
                .highlight_symbol("» ");

            let instruction_para = Paragraph::new(instructions.clone())
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray));

            frame.render_widget(title, layout[0]);
            frame.render_widget(path, layout[1]);
            frame.render_widget(list, layout[2]);
            frame.render_widget(instruction_para, layout[3]);
        })?;

        Ok(())
    }

    fn cleanup(&mut self) -> Result<()> {
        if self.cleaned_up {
            return Ok(());
        }

        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        self.cleaned_up = true;
        Ok(())
    }
}

impl Drop for KeyChordNavigator {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn demo_tree() -> KeyChordNode {
    use KeyChordChild::{Leaf, Node};

    KeyChordNode::new(
        "Root",
        vec![
            KeyChord::new(
                "Window Management",
                KeyCode::Char('w'),
                Node(KeyChordNode::new(
                    "Windows",
                    vec![
                        KeyChord::new(
                            "Split Horizontally",
                            KeyCode::Char('h'),
                            Leaf(KeyChordAction::new("window.split.horizontal")),
                        ),
                        KeyChord::new(
                            "Split Vertically",
                            KeyCode::Char('v'),
                            Leaf(KeyChordAction::new("window.split.vertical")),
                        ),
                        KeyChord::new(
                            "Focus Next",
                            KeyCode::Char('n'),
                            Leaf(KeyChordAction::new("window.focus.next")),
                        ),
                        KeyChord::new(
                            "Layouts",
                            KeyCode::Char('l'),
                            Node(KeyChordNode::new(
                                "Layouts",
                                vec![
                                    KeyChord::new(
                                        "Tile",
                                        KeyCode::Char('t'),
                                        Leaf(KeyChordAction::new("window.layout.tile")),
                                    ),
                                    KeyChord::new(
                                        "Monocle",
                                        KeyCode::Char('m'),
                                        Leaf(KeyChordAction::new("window.layout.monocle")),
                                    ),
                                ],
                            )),
                        ),
                    ],
                )),
            ),
            KeyChord::new(
                "Applications",
                KeyCode::Char('a'),
                Node(KeyChordNode::new(
                    "Applications",
                    vec![
                        KeyChord::new(
                            "Browser",
                            KeyCode::Char('b'),
                            Leaf(KeyChordAction::new("app.launch.browser")),
                        ),
                        KeyChord::new(
                            "File Manager",
                            KeyCode::Char('f'),
                            Leaf(KeyChordAction::new("app.launch.files")),
                        ),
                        KeyChord::new(
                            "Editors",
                            KeyCode::Char('e'),
                            Node(KeyChordNode::new(
                                "Editors",
                                vec![
                                    KeyChord::new(
                                        "Neovim",
                                        KeyCode::Char('n'),
                                        Leaf(KeyChordAction::new("app.launch.neovim")),
                                    ),
                                    KeyChord::new(
                                        "VS Code",
                                        KeyCode::Char('v'),
                                        Leaf(KeyChordAction::new("app.launch.vscode")),
                                    ),
                                ],
                            )),
                        ),
                    ],
                )),
            ),
            KeyChord::new(
                "System",
                KeyCode::Char('s'),
                Node(KeyChordNode::new(
                    "System",
                    vec![
                        KeyChord::new(
                            "Toggle Wi-Fi",
                            KeyCode::Char('w'),
                            Leaf(KeyChordAction::new("system.wifi.toggle")),
                        ),
                        KeyChord::new(
                            "Restart",
                            KeyCode::Char('r'),
                            Leaf(KeyChordAction::new("system.restart")),
                        ),
                    ],
                )),
            ),
        ],
    )
}

fn key_label(code: &KeyCode) -> String {
    match code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pgup".to_string(),
        KeyCode::PageDown => "pgdn".to_string(),
        KeyCode::Delete => "del".to_string(),
        KeyCode::Insert => "ins".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => "?".to_string(),
    }
}
