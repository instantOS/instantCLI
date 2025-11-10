use std::collections::{BTreeMap, BTreeSet};
use std::io::stdout;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::menu_utils::{KeyChord, KeyChordAction, KeyChordChild, KeyChordNode};

const POLL_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChordSpec {
    sequence: String,
    description: String,
}

pub fn run_chord_selection(chord_specs: &[String]) -> Result<Option<String>> {
    if chord_specs.is_empty() {
        return Err(anyhow!("Provide at least one chord specification"));
    }

    let parsed_specs = parse_chord_specs(chord_specs)?;
    let tree = build_chord_tree(&parsed_specs)?;

    let mut navigator = KeyChordNavigator::new(tree)?;
    navigator.run()
}

pub fn run_chord_command(chord_specs: &[String]) -> Result<i32> {
    match run_chord_selection(chord_specs)? {
        Some(sequence) => {
            println!("{sequence}");
            Ok(0)
        }
        None => Ok(1),
    }
}

fn parse_chord_specs(raw: &[String]) -> Result<Vec<ChordSpec>> {
    let mut specs = Vec::with_capacity(raw.len());
    let mut seen = BTreeSet::new();

    for entry in raw {
        let (sequence, description) = entry
            .split_once(':')
            .ok_or_else(|| anyhow!("Chord '{entry}' must be in KEY:DESCRIPTION format"))?;

        let sequence = sequence.trim();
        if sequence.is_empty() {
            return Err(anyhow!("Chord '{entry}' must specify at least one key"));
        }

        if !sequence.chars().all(|ch| !ch.is_control()) {
            return Err(anyhow!(
                "Chord '{entry}' contains control characters, which are not supported"
            ));
        }

        let description = description.trim();
        if description.is_empty() {
            return Err(anyhow!(
                "Chord '{entry}' must include a non-empty description after ':'"
            ));
        }

        if !seen.insert(sequence.to_string()) {
            return Err(anyhow!("Chord '{sequence}' provided multiple times"));
        }

        specs.push(ChordSpec {
            sequence: sequence.to_string(),
            description: description.to_string(),
        });
    }

    Ok(specs)
}

fn build_chord_tree(specs: &[ChordSpec]) -> Result<KeyChordNode> {
    let mut nodes: BTreeMap<String, NodeBuilder> = BTreeMap::new();
    nodes.insert(String::new(), NodeBuilder::default());

    for spec in specs {
        let mut prefix = String::new();
        for ch in spec.sequence.chars() {
            let parent_prefix = prefix.clone();
            prefix.push(ch);

            nodes
                .entry(parent_prefix.clone())
                .or_default()
                .add_child(ch, prefix.clone());

            nodes.entry(prefix.clone()).or_default();
        }
    }

    let mut has_leaf = false;
    for spec in specs {
        let node = nodes
            .get_mut(&spec.sequence)
            .context("Internal error creating chord tree")?;
        node.description = Some(spec.description.clone());
        if node.children.is_empty() {
            node.action = Some(spec.sequence.clone());
            has_leaf = true;
        }
    }

    if !has_leaf {
        return Err(anyhow!(
            "Chord list must include at least one complete chord (without further children)"
        ));
    }

    for (sequence, node) in nodes.iter_mut() {
        if node.description.is_none() {
            if sequence.is_empty() {
                node.description = Some("Chord Menu".to_string());
            } else {
                node.description = Some(sequence.clone());
            }
        }
    }

    Ok(build_node("", &nodes))
}

#[derive(Default, Debug, Clone)]
struct NodeBuilder {
    description: Option<String>,
    children: BTreeMap<char, String>,
    action: Option<String>,
}

impl NodeBuilder {
    fn add_child(&mut self, key: char, target: String) {
        self.children.entry(key).or_insert(target);
    }
}

fn build_node(prefix: &str, nodes: &BTreeMap<String, NodeBuilder>) -> KeyChordNode {
    let builder = nodes.get(prefix).expect("missing node");
    let mut chords = Vec::with_capacity(builder.children.len());

    for (ch, child_prefix) in builder.children.iter() {
        let child_builder = nodes
            .get(child_prefix)
            .expect("missing child node during build");
        let key = KeyCode::Char(*ch);
        let label = child_builder
            .description
            .clone()
            .unwrap_or_else(|| child_prefix.clone());

        let child = if let Some(action) = &child_builder.action {
            KeyChordChild::Leaf(KeyChordAction::new(action.clone()))
        } else {
            KeyChordChild::Node(build_node(child_prefix, nodes))
        };

        chords.push(KeyChord::new(label, key, child));
    }

    KeyChordNode::new(
        builder
            .description
            .clone()
            .unwrap_or_else(|| "Chord Menu".to_string()),
        chords,
    )
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
                        KeyCode::Char('q') if key_event.modifiers.is_empty() => break,
                        code => {
                            if key_event.modifiers.is_empty()
                                && let Some(chord) = self.current_node.find_chord(&code).cloned()
                            {
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
                                        self.history
                                            .push((self.current_node.clone(), self.path.clone()));
                                        self.current_node = node;
                                        self.path.push(breadcrumb);
                                        needs_redraw = true;
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
                    "Chord Picker",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tree_with_parent_descriptions() {
        let specs = vec![
            "aa:Twice A".to_string(),
            "ab:A then B".to_string(),
            "a:A group".to_string(),
        ];

        let parsed = parse_chord_specs(&specs).unwrap();
        let tree = build_chord_tree(&parsed).unwrap();

        assert_eq!(tree.description, "Chord Menu");
        assert_eq!(tree.chords().len(), 1);

        let chord = &tree.chords()[0];
        assert_eq!(chord.description, "A group");

        match &chord.child {
            KeyChordChild::Node(node) => {
                assert_eq!(node.description, "A group");
                assert_eq!(node.chords().len(), 2);

                let ids: Vec<_> = node
                    .chords()
                    .iter()
                    .filter_map(|c| match &c.child {
                        KeyChordChild::Leaf(action) => {
                            Some((c.description.clone(), action.id.clone()))
                        }
                        _ => None,
                    })
                    .collect();

                assert_eq!(ids.len(), 2);
                assert!(ids.contains(&("Twice A".to_string(), "aa".to_string())));
                assert!(ids.contains(&("A then B".to_string(), "ab".to_string())));
            }
            _ => panic!("Expected node child"),
        }
    }

    #[test]
    fn errors_on_invalid_format() {
        let specs = vec!["invalid".to_string()];
        assert!(parse_chord_specs(&specs).is_err());
    }

    #[test]
    fn infers_parent_nodes() {
        let specs = vec!["ab:Child".to_string()];
        let parsed = parse_chord_specs(&specs).unwrap();
        let tree = build_chord_tree(&parsed).unwrap();

        assert_eq!(tree.description, "Chord Menu");
        assert_eq!(tree.chords().len(), 1);

        let chord = &tree.chords()[0];
        assert_eq!(key_label(&chord.key), "a");
        assert_eq!(chord.description, "a");

        match &chord.child {
            KeyChordChild::Node(node) => {
                assert_eq!(node.description, "a");
                assert_eq!(node.chords().len(), 1);
                match &node.chords()[0].child {
                    KeyChordChild::Leaf(action) => assert_eq!(action.id, "ab"),
                    _ => panic!("Expected leaf"),
                }
            }
            _ => panic!("Expected node"),
        }
    }

    #[test]
    fn rejects_duplicate_sequences() {
        let specs = vec!["aa:First".to_string(), "aa:Second".to_string()];
        assert!(parse_chord_specs(&specs).is_err());
    }
}
