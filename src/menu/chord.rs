use std::collections::{BTreeMap, BTreeSet};
use std::io::stdout;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use crate::menu_utils::{KeyChord, KeyChordAction, KeyChordChild, KeyChordNode};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;

const POLL_TIMEOUT: Duration = Duration::from_millis(200);

/// Converts a Catppuccin hex color (`#RRGGBB`) into a ratatui `Color::Rgb`.
fn rgb(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color::Reset;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    Color::Rgb(r, g, b)
}

/// Outcome of activating (clicking / pressing Enter on) a chord entry.
enum Activation {
    /// Stay in the menu (e.g. descended into a group).
    Continue,
    /// Leave the menu with a result (`Some`) or cancellation (`None`).
    Exit(Option<String>),
}

/// Display width of a string (cells), accounting for nerd-font/wide glyphs.
fn str_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// Truncate `s` to fit within `max` display cells, appending an ellipsis when
/// characters are dropped.
fn truncate_to_width(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if str_width(s) <= max {
        return s.to_string();
    }
    let ellipsis = '…';
    let ellipsis_w = unicode_width::UnicodeWidthChar::width(ellipsis).unwrap_or(1);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > max.saturating_sub(ellipsis_w) {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push(ellipsis);
    out
}

/// A chord description split into its visual parts.
///
/// Assist descriptions arrive as `{icon} {Name}: {Detail}`. Descriptions built
/// from direct CLI input may omit the icon and/or the colon; both cases are
/// handled gracefully.
struct DescriptionParts<'a> {
    icon: Option<char>,
    name: &'a str,
    detail: &'a str,
}

fn parse_description(desc: &str) -> DescriptionParts<'_> {
    let mut chars = desc.chars();
    let icon = match (chars.next(), chars.next()) {
        // A non-ASCII glyph followed by a space is treated as the nerd-font icon.
        (Some(first), Some(' ')) if !first.is_ascii() => Some(first),
        _ => None,
    };
    let after_icon = match icon {
        Some(c) => &desc[c.len_utf8()..],
        None => desc,
    };
    let rest = after_icon.trim_start();
    let (name, detail) = match rest.split_once(':') {
        Some((n, d)) => (n.trim(), d.trim()),
        None => (rest, ""),
    };
    DescriptionParts { icon, name, detail }
}

/// Horizontal layout for chord rows. Columns are aligned when there is room;
/// on narrow terminals the layout degrades to compact (chevron right after the
/// detail, no truncation).
struct RowLayout {
    key_width: usize,
    name_width: usize,
    /// Width of the icon-badge column (0 when no row carries an icon).
    badge_width: usize,
    /// Absolute column where the detail text begins.
    detail_start: usize,
    /// When `Some`, group chevrons anchor to this right-edge column and details
    /// are truncated to fit. When `None`, the layout is compact.
    chevron_x: Option<usize>,
    /// Total row width (for trailing hover fill).
    width: usize,
}

/// Inner padding (each side) of the colored icon badge.
const BADGE_PAD: usize = 2;

impl RowLayout {
    /// Compute a layout for the given chords within `width` cells.
    fn compute(chords: &[KeyChord], width: u16) -> Self {
        let key_width = chords
            .iter()
            .map(|c| str_width(&key_label(&c.key)))
            .max()
            .unwrap_or(0)
            .clamp(2, 6);

        let name_width = chords
            .iter()
            .map(|c| str_width(parse_description(&c.description).name))
            .max()
            .unwrap_or(0);

        let has_badges = chords
            .iter()
            .any(|c| parse_description(&c.description).icon.is_some());
        let badge_width = if has_badges { 1 + BADGE_PAD * 2 } else { 0 };

        // Columns before the name:
        // pointer(1) + gap(1) + key + gap(1) + badge + gap(1 if badge present).
        let pre_name = 1 + 1 + key_width + 1 + badge_width + usize::from(has_badges);
        let detail_start = pre_name + name_width + 2;

        let has_groups = chords
            .iter()
            .any(|c| matches!(c.child, KeyChordChild::Node(_)));

        // Only anchor a chevron column when there is comfortable room for detail.
        let chevron_x = if has_groups && (width as usize) > detail_start + 10 {
            Some((width as usize).saturating_sub(2))
        } else {
            None
        };

        RowLayout {
            key_width,
            name_width,
            badge_width,
            detail_start,
            chevron_x,
            width: width as usize,
        }
    }

    /// Available cells for the detail text before the chevron column.
    fn detail_avail(&self) -> usize {
        match self.chevron_x {
            Some(cx) => cx.saturating_sub(self.detail_start).saturating_sub(1),
            None => 0,
        }
    }
}

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

    KeyChordNode::new(chords)
}

/// Pure navigation state for the chord tree, independent of the terminal.
///
/// Keeping the cursor (`selected`) and the back-stack here — rather than on the
/// terminal-coupled navigator — makes descent/back logic unit-testable. The
/// navigator mirrors `selected` into a `ListState` purely for rendering, which
/// is also where the scroll offset lives.
///
/// The back-stack remembers the index of the chord each level was descended
/// from, so going back restores the cursor onto the entry the user just entered
/// instead of dropping back to "nothing selected".
struct NavState {
    current_node: KeyChordNode,
    path: Vec<String>,
    /// Back-stack: (parent node, parent breadcrumb path, index of the chord
    /// descended from at the parent level).
    history: Vec<(KeyChordNode, Vec<String>, usize)>,
    selected: Option<usize>,
}

impl NavState {
    fn new(root: KeyChordNode) -> Self {
        Self {
            current_node: root,
            path: Vec::new(),
            history: Vec::new(),
            selected: None,
        }
    }

    /// Descend into a child node, recording the parent (and the index of the
    /// chord we came from) so `go_back` can restore the cursor there.
    fn descend(&mut self, from_idx: usize, description: String, node: KeyChordNode) {
        let parent = std::mem::replace(&mut self.current_node, node);
        self.history.push((parent, self.path.clone(), from_idx));
        self.path.push(description);
        // A fresh level starts with no selection so the first cursor-down lands
        // on item 0 rather than pre-selecting an arbitrary row.
        self.selected = None;
    }

    /// Navigate back one level. Returns `true` if it moved back, `false` at the
    /// root. Restores the cursor to the chord the user descended from.
    fn go_back(&mut self) -> bool {
        if let Some((node, path, idx)) = self.history.pop() {
            self.current_node = node;
            self.path = path;
            self.selected = Some(idx);
            true
        } else {
            false
        }
    }

    /// Move the cursor by `delta` positions (clamped). When nothing is selected
    /// yet the cursor starts "before" the first item so the first step down
    /// lands on item 0.
    fn move_cursor(&mut self, delta: i32) {
        let len = self.current_node.chords.len();
        if len == 0 {
            return;
        }
        let current = self.selected.map(|i| i as i32).unwrap_or(-1);
        let next = (current + delta).clamp(0, len as i32 - 1) as usize;
        self.selected = Some(next);
    }

    /// Activate the chord at `idx`. Descends into groups, exits on leaves.
    fn activate_index(&mut self, idx: usize) -> Activation {
        let Some(chord) = self.current_node.chords.get(idx).cloned() else {
            return Activation::Continue;
        };
        match chord.child {
            KeyChordChild::Leaf(action) => Activation::Exit(Some(action.id)),
            KeyChordChild::Node(node) => {
                self.descend(idx, chord.description, node);
                Activation::Continue
            }
        }
    }

    /// Activate the chord bound to `key`, if any. Used for letter-key entry.
    fn activate_by_key(&mut self, key: &KeyCode) -> Option<Activation> {
        let idx = self
            .current_node
            .chords
            .iter()
            .position(|c| &c.key == key)?;
        Some(self.activate_index(idx))
    }
}

struct KeyChordNavigator {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    nav: NavState,
    /// Render mirror: carries the scroll offset (updated by the list widget);
    /// its `selected` is refreshed from `nav.selected` on every draw.
    list_state: ListState,
    /// List content area from the last draw, used to map mouse rows to items.
    list_area: Rect,
    /// Breadcrumb row (absolute y) from the last draw, used for click detection.
    breadcrumb_row: u16,
    /// Whether the mouse currently hovers over the (clickable) breadcrumb row.
    breadcrumb_hovered: bool,
    cleaned_up: bool,
}

impl KeyChordNavigator {
    fn new(root: KeyChordNode) -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        terminal.hide_cursor()?;

        Ok(Self {
            terminal,
            nav: NavState::new(root),
            list_state: ListState::default(),
            list_area: Rect::default(),
            breadcrumb_row: 0,
            breadcrumb_hovered: false,
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
                            if self.nav_go_back() {
                                needs_redraw = true;
                            } else {
                                break;
                            }
                        }
                        // Cursor navigation (complements hover / scroll).
                        // NOTE: only non-letter keys are used here — letters are
                        // valid chord keys and must fall through to chord lookup.
                        KeyCode::Up => {
                            self.nav.move_cursor(-1);
                            needs_redraw = true;
                        }
                        KeyCode::Down => {
                            self.nav.move_cursor(1);
                            needs_redraw = true;
                        }
                        KeyCode::PageUp => {
                            self.nav.move_cursor(-5);
                            needs_redraw = true;
                        }
                        KeyCode::PageDown => {
                            self.nav.move_cursor(5);
                            needs_redraw = true;
                        }
                        KeyCode::Left => {
                            // Go back one level (no quit at root).
                            if self.nav_go_back() {
                                needs_redraw = true;
                            }
                        }
                        KeyCode::Enter | KeyCode::Right => {
                            let idx = self.nav.selected.unwrap_or(0);
                            match self.nav.activate_index(idx) {
                                Activation::Exit(result) => {
                                    self.cleanup()?;
                                    return Ok(result);
                                }
                                Activation::Continue => {
                                    self.breadcrumb_hovered = false;
                                    needs_redraw = true;
                                }
                            }
                        }
                        code => {
                            if key_event.modifiers.is_empty()
                                && let Some(activation) = self.nav.activate_by_key(&code)
                            {
                                match activation {
                                    Activation::Exit(result) => {
                                        self.cleanup()?;
                                        return Ok(result);
                                    }
                                    Activation::Continue => {
                                        self.breadcrumb_hovered = false;
                                        needs_redraw = true;
                                    }
                                }
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Moved => {
                        let breadcrumb_hovered = self.is_over_breadcrumb(mouse.row);
                        let mut changed = breadcrumb_hovered != self.breadcrumb_hovered;
                        self.breadcrumb_hovered = breadcrumb_hovered;

                        // Only take over the highlight when actually hovering an
                        // item; otherwise leave a keyboard/scroll selection intact.
                        if let Some(idx) = self.list_index_at(mouse.row)
                            && self.nav.selected != Some(idx)
                        {
                            self.nav.selected = Some(idx);
                            changed = true;
                        }

                        if changed {
                            needs_redraw = true;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if self.is_over_breadcrumb(mouse.row) && !self.nav.path.is_empty() {
                            self.nav_go_back();
                            needs_redraw = true;
                        } else if let Some(idx) = self.list_index_at(mouse.row) {
                            match self.nav.activate_index(idx) {
                                Activation::Exit(result) => {
                                    self.cleanup()?;
                                    return Ok(result);
                                }
                                Activation::Continue => {
                                    self.breadcrumb_hovered = false;
                                    needs_redraw = true;
                                }
                            }
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        self.nav.move_cursor(1);
                        needs_redraw = true;
                    }
                    MouseEventKind::ScrollUp => {
                        self.nav.move_cursor(-1);
                        needs_redraw = true;
                    }
                    _ => {}
                },
                Event::Resize(_, _) => needs_redraw = true,
                _ => {}
            }
        }

        self.cleanup()?;
        Ok(None)
    }

    /// Go back one level, clearing stale breadcrumb hover when the view changes.
    fn nav_go_back(&mut self) -> bool {
        let moved = self.nav.go_back();
        if moved {
            self.breadcrumb_hovered = false;
        }
        moved
    }

    /// Map an absolute terminal row to a chord index (accounting for scroll).
    fn list_index_at(&self, row: u16) -> Option<usize> {
        let area = self.list_area;
        if area.height == 0 || row < area.y || row >= area.y + area.height {
            return None;
        }
        let index = self.list_state.offset() + (row - area.y) as usize;
        (index < self.nav.current_node.chords.len()).then_some(index)
    }

    fn is_over_breadcrumb(&self, row: u16) -> bool {
        self.breadcrumb_row != 0 && row == self.breadcrumb_row
    }

    fn draw(&mut self) -> Result<()> {
        let node = self.nav.current_node.clone();
        let path = self.nav.path.clone();
        let breadcrumb_hovered = self.breadcrumb_hovered;
        let mut state = self.list_state;
        // Mirror the navigation selection into the list state for rendering;
        // the widget updates the scroll offset, which we keep below.
        state.select(self.nav.selected);
        let mut list_area = Rect::default();
        let mut breadcrumb_row = 0u16;

        self.terminal.draw(|frame| {
            let (la, br) = render_chord_ui(frame, &node, &path, breadcrumb_hovered, &mut state);
            list_area = la;
            breadcrumb_row = br;
        })?;

        self.list_state = state;
        self.list_area = list_area;
        self.breadcrumb_row = breadcrumb_row;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<()> {
        if self.cleaned_up {
            return Ok(());
        }

        disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
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

/// Render a single frame. Returns the list content area and breadcrumb row y
/// (used by the navigator for mouse hit-testing).
fn render_chord_ui(
    frame: &mut ratatui::Frame,
    node: &KeyChordNode,
    path: &[String],
    breadcrumb_hovered: bool,
    state: &mut ListState,
) -> (Rect, u16) {
    let area = frame.area();
    frame.render_widget(Clear, area);

    let [header_area, list_area, footer_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // title, spacer, breadcrumb, spacer
            Constraint::Min(5),    // chord list
            Constraint::Length(1), // instructions
        ])
        .areas(area);

    let [title_row, _title_gap, breadcrumb_row_area, _list_gap] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(header_area);

    frame.render_widget(title_line(), title_row);
    frame.render_widget(
        breadcrumb_line(path, breadcrumb_hovered),
        breadcrumb_row_area,
    );

    let selected = state.selected();
    let layout = RowLayout::compute(&node.chords, list_area.width);
    let items: Vec<ListItem> = node
        .chords
        .iter()
        .enumerate()
        .map(|(idx, chord)| ListItem::new(chord_line(chord, selected == Some(idx), &layout)))
        .collect();

    let list = List::new(items).style(Style::default().fg(rgb(colors::TEXT)));

    frame.render_stateful_widget(list, list_area, state);
    frame.render_widget(footer_line(), footer_area);

    (list_area, breadcrumb_row_area.y)
}

fn title_line() -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            NerdFont::Keyboard.to_string(),
            Style::default()
                .fg(rgb(colors::MAUVE))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "Chord Menu",
            Style::default()
                .fg(rgb(colors::TEXT))
                .add_modifier(Modifier::BOLD),
        ),
    ]))
}

fn breadcrumb_line(path: &[String], hovered: bool) -> Paragraph<'static> {
    let mut spans: Vec<Span<'static>> = vec![
        Span::raw("  "),
        Span::styled(
            NerdFont::Home.to_string(),
            Style::default()
                .fg(rgb(colors::MAUVE))
                .add_modifier(Modifier::BOLD),
        ),
    ];

    let separator = Style::default().fg(rgb(colors::OVERLAY1));

    for (idx, segment) in path.iter().enumerate() {
        let is_current = idx == path.len() - 1;
        let color = if is_current {
            colors::TEXT
        } else if hovered {
            colors::LAVENDER
        } else {
            colors::SUBTEXT1
        };
        let mut style = Style::default().fg(rgb(color));
        if is_current {
            style = style.add_modifier(Modifier::BOLD);
        }
        spans.push(Span::raw("  "));
        spans.push(Span::styled(NerdFont::ChevronRight.to_string(), separator));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(segment.clone(), style));
    }

    Paragraph::new(Line::from(spans))
}

fn chord_line(chord: &KeyChord, hovered: bool, layout: &RowLayout) -> Line<'static> {
    // Hover background is baked into each span (the badge keeps its own color),
    // so the colored pill survives highlighting. A trailing fill completes the
    // row background to the full width.
    let bg = hovered.then(|| rgb(colors::SURFACE0));
    let paint = |mut style: Style| -> Style {
        if let Some(c) = bg {
            style = style.bg(c);
        }
        style
    };
    let gap = |n: usize| -> Span<'static> { Span::styled(" ".repeat(n), paint(Style::default())) };

    let pointer = Span::styled(
        "▌",
        paint(
            Style::default()
                .fg(rgb(colors::ROSEWATER))
                .add_modifier(Modifier::BOLD),
        ),
    );
    // Non-hovered rows use a blank pointer column for alignment.
    let pointer = if hovered { pointer } else { gap(1) };

    let (is_group, badge_color) = match &chord.child {
        KeyChordChild::Node(_) => (true, colors::SAPPHIRE),
        KeyChordChild::Leaf(_) => (false, colors::GREEN),
    };
    let key_color = if is_group { colors::SKY } else { colors::GREEN };

    // Right-align the key in the fixed key column.
    let key = Span::styled(
        format!("{:>w$}", key_label(&chord.key), w = layout.key_width),
        paint(
            Style::default()
                .fg(rgb(key_color))
                .add_modifier(Modifier::BOLD),
        ),
    );

    let parts = parse_description(&chord.description);

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut col = 0usize;

    spans.push(pointer);
    col += 1;
    spans.push(gap(1));
    col += 1;
    spans.push(key);
    col += layout.key_width;
    spans.push(gap(1));
    col += 1;

    // Icon badge — a colored pill (keeps its bg on hover); or an empty slot.
    if layout.badge_width > 0 {
        match parts.icon {
            Some(icon) => spans.push(Span::styled(
                format!("{}{}{}", " ".repeat(BADGE_PAD), icon, " ".repeat(BADGE_PAD)),
                Style::default().fg(rgb(colors::CRUST)).bg(rgb(badge_color)),
            )),
            None => spans.push(gap(layout.badge_width)),
        }
        col += layout.badge_width;
        spans.push(gap(1));
        col += 1;
    }

    // Name — left-aligned in its column (colon omitted).
    let name_pad = layout.name_width.saturating_sub(str_width(parts.name));
    spans.push(Span::styled(
        format!("{}{}", parts.name, " ".repeat(name_pad)),
        paint(
            Style::default()
                .fg(rgb(colors::TEXT))
                .add_modifier(Modifier::BOLD),
        ),
    ));
    col += layout.name_width;

    // Detail — separate, muted column.
    if !parts.detail.is_empty() {
        spans.push(gap(2));
        col += 2;
        let detail = match layout.chevron_x {
            Some(_) => truncate_to_width(parts.detail, layout.detail_avail()),
            None => parts.detail.to_string(),
        };
        col += str_width(&detail);
        spans.push(Span::styled(
            detail,
            paint(Style::default().fg(rgb(colors::OVERLAY2))),
        ));
    }

    // Chevron — aligned to the right-edge column for groups.
    if is_group {
        match layout.chevron_x {
            Some(cx) => {
                let pad = cx.saturating_sub(col);
                spans.push(gap(pad));
                col += pad;
            }
            None => {
                spans.push(gap(2));
                col += 2;
            }
        }
        spans.push(Span::styled(
            NerdFont::ChevronRight.to_string(),
            paint(Style::default().fg(rgb(colors::OVERLAY1))),
        ));
        col += 1;
    }

    // Trailing fill so the hover background spans the full row width.
    if hovered {
        let pad = layout.width.saturating_sub(col);
        spans.push(gap(pad));
    }

    Line::from(spans)
}

fn footer_line() -> Paragraph<'static> {
    let key_hint = Style::default().fg(rgb(colors::SKY));
    let accent = Style::default().fg(rgb(colors::PEACH));
    let dim = Style::default().fg(rgb(colors::OVERLAY1));

    Paragraph::new(Line::from(vec![
        Span::styled("Hover", accent),
        Span::styled(" / ", dim),
        Span::styled("click", accent),
        Span::styled(" or press a ", dim),
        Span::styled("key", key_hint),
        Span::styled("   ·   ", dim),
        Span::styled("Esc", key_hint),
        Span::styled(" / ", dim),
        Span::styled("Backspace", key_hint),
        Span::styled(" back", dim),
        Span::styled("   ·   ", dim),
        Span::styled("Ctrl+C", key_hint),
        Span::styled(" quit", dim),
    ]))
    .alignment(Alignment::Center)
    .style(Style::default().fg(rgb(colors::OVERLAY2)))
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

        assert_eq!(tree.chords.len(), 1);

        let chord = &tree.chords[0];
        assert_eq!(chord.description, "A group");

        match &chord.child {
            KeyChordChild::Node(node) => {
                assert_eq!(node.chords.len(), 2);

                let ids: Vec<_> = node
                    .chords
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

        assert_eq!(tree.chords.len(), 1);

        let chord = &tree.chords[0];
        assert_eq!(key_label(&chord.key), "a");
        assert_eq!(chord.description, "a");

        match &chord.child {
            KeyChordChild::Node(node) => {
                assert_eq!(node.chords.len(), 1);
                match &node.chords[0].child {
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

    #[test]
    fn go_back_restores_cursor_to_descended_entry() {
        // Root has a single group 'a' (index 0) whose child is leaf 'ab'.
        let tree = build_chord_tree(&parse_chord_specs(&["ab:Leaf".to_string()]).unwrap()).unwrap();
        let mut nav = NavState::new(tree);

        // Descend into the group; a fresh level starts with no selection.
        assert!(matches!(nav.activate_index(0), Activation::Continue));
        assert_eq!(nav.selected, None);

        // Going back must land the cursor on the entry we just entered.
        assert!(nav.go_back());
        assert_eq!(nav.selected, Some(0));
    }

    #[test]
    fn go_back_restores_nonzero_index() {
        // Two top-level groups so we can descend from index 1.
        let tree = build_chord_tree(
            &parse_chord_specs(&["ab:First".to_string(), "cd:Second".to_string()]).unwrap(),
        )
        .unwrap();
        let mut nav = NavState::new(tree);

        assert!(matches!(nav.activate_index(1), Activation::Continue));
        assert_eq!(nav.selected, None);

        assert!(nav.go_back());
        assert_eq!(nav.selected, Some(1));
    }

    #[test]
    fn go_back_restores_after_key_descent() {
        // The letter-key entry path must also remember its source index.
        let tree = build_chord_tree(
            &parse_chord_specs(&["ab:First".to_string(), "cd:Second".to_string()]).unwrap(),
        )
        .unwrap();
        let mut nav = NavState::new(tree);

        // Press 'c' to descend into the second group (index 1).
        assert!(matches!(
            nav.activate_by_key(&KeyCode::Char('c')),
            Some(Activation::Continue)
        ));
        assert!(nav.go_back());
        assert_eq!(nav.selected, Some(1));
    }

    #[test]
    fn descend_starts_new_level_unselected() {
        // After descending, the new level must have no preselected entry so the
        // first cursor-down lands on item 0 (regression guard).
        let tree = build_chord_tree(
            &parse_chord_specs(&["ab:First".to_string(), "cd:Second".to_string()]).unwrap(),
        )
        .unwrap();
        let mut nav = NavState::new(tree);
        nav.move_cursor(1); // root: nothing -> select index 0
        assert_eq!(nav.selected, Some(0));

        assert!(matches!(nav.activate_index(0), Activation::Continue));
        assert_eq!(nav.selected, None);
    }

    #[test]
    fn go_back_at_root_is_noop() {
        let tree = build_chord_tree(&parse_chord_specs(&["ab:Leaf".to_string()]).unwrap()).unwrap();
        let mut nav = NavState::new(tree);
        nav.move_cursor(1);
        assert_eq!(nav.selected, Some(0));

        assert!(!nav.go_back()); // at root: nothing to pop
        assert_eq!(nav.selected, Some(0)); // selection untouched
    }

    #[test]
    fn q_remains_available_as_a_chord_key() {
        let tree =
            build_chord_tree(&parse_chord_specs(&["q:QR action".to_string()]).unwrap()).unwrap();
        let mut nav = NavState::new(tree);

        assert!(matches!(
            nav.activate_by_key(&KeyCode::Char('q')),
            Some(Activation::Exit(Some(id))) if id == "q"
        ));
    }
}
