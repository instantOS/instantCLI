//! FZF wrapper and selection logic

use anyhow::{Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use crossbeam_channel::{Receiver, TryRecvError, bounded, select};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use super::preview::MixedPreviewContent;
use super::preview::PreviewStrategy;
use super::preview::PreviewUtils;
use super::types::ItemDisplayData;
use super::types::*;
use super::utils::{handle_old_fzf_error, log_fzf_failure};
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

/// Named parts extracted from `FzfBuilder` for constructing `FzfWrapper`.
pub(crate) struct FzfWrapperParts {
    pub multi_select: bool,
    pub prompt: Option<String>,
    pub header: Option<Header>,
    pub additional_args: Vec<String>,
    pub initial_cursor: Option<InitialCursor>,
    pub responsive_layout: bool,
}

/// Classify fzf's process status without conflating cancellation and failure.
///
/// Exit 1 means "no match" and is interpreted by each caller from stdout.
/// Exit 130/143 represents an interrupted interaction. Every other non-zero
/// status is an operational failure.
pub(crate) fn fzf_was_cancelled(result: &std::process::Output) -> Result<bool> {
    match result.status.code() {
        Some(0 | 1) => Ok(false),
        Some(130 | 143) => Ok(true),
        code => {
            handle_old_fzf_error(&result.stderr);
            if crate::ui::is_debug_enabled() {
                log_fzf_failure(&result.stderr, code, |code, message| {
                    crate::ui::emit(crate::ui::Level::Debug, code, message, None);
                });
            }
            let stderr = String::from_utf8_lossy(&result.stderr);
            let detail = stderr.trim();
            if detail.is_empty() {
                Err(anyhow!("fzf exited with status {}", result.status))
            } else {
                Err(anyhow!(
                    "fzf exited with status {}: {detail}",
                    result.status
                ))
            }
        }
    }
}

// ============================================================================
// Helper functions for FzfWrapper::select
// ============================================================================

/// Build a lookup map from fzf_key to item, and collect display lines with keys and search keywords.
/// All items (including non-selectable separators) are included in item_map so that
/// `parse_fzf_output` can return them — callers like `select_menu` handle separator
/// selections by re-launching the menu.
fn build_item_map<T: FzfSelectable + Clone>(
    items: &[T],
) -> (HashMap<String, T>, Vec<ItemDisplayData>) {
    let mut item_map: HashMap<String, T> = HashMap::new();
    let mut display_data = Vec::new();

    for item in items {
        let display_text = item.fzf_display_text();
        let key = item.fzf_key();
        let keywords = item.fzf_search_keywords().join(" ");
        let is_selectable = item.fzf_is_selectable();
        display_data.push(ItemDisplayData {
            display_text,
            key: key.clone(),
            keywords,
            is_selectable,
        });
        item_map.insert(key, item.clone());
    }

    (item_map, display_data)
}

/// Calculate the initial cursor position based on configuration.
fn calculate_cursor_position(
    initial_cursor: &Option<InitialCursor>,
    item_count: usize,
) -> Option<usize> {
    match initial_cursor {
        Some(InitialCursor::Index(index)) if item_count > 0 => Some((*index).min(item_count - 1)),
        _ => None,
    }
}

/// Calculate cursor position for separator mode, ensuring it never lands on a separator.
fn calculate_separator_aware_cursor(
    initial_cursor: &Option<InitialCursor>,
    display_data: &[ItemDisplayData],
) -> Option<usize> {
    if display_data.is_empty() {
        return None;
    }

    let requested = match initial_cursor {
        Some(InitialCursor::Index(index)) => Some((*index).min(display_data.len() - 1)),
        _ => None,
    };

    let pos = requested.unwrap_or(0);

    // If the requested position is selectable, use it
    if display_data[pos].is_selectable {
        return Some(pos);
    }

    // Search forward for the nearest selectable item
    if let Some(fwd) = display_data[pos..].iter().position(|d| d.is_selectable) {
        return Some(pos + fwd);
    }

    // Search backward
    display_data[..pos].iter().rposition(|d| d.is_selectable)
}

/// Render the dimmed header hint line for a menu's registered keybinds.
fn keybind_hint_line<A>(keybinds: &[MenuKeybind<A>]) -> String {
    let dim = hex_to_ansi_fg(colors::OVERLAY0);
    let reset = "\x1b[0m";
    let hints = keybinds
        .iter()
        .map(|bind| format!("{} {}", bind.key, bind.label))
        .collect::<Vec<_>>()
        .join("   ");
    format!("{dim}{hints}{reset}")
}

/// Register the menu's keybinds with fzf: one `print(token)+accept` binding
/// per key, so pressing it terminates fzf with the token on stdout ahead of
/// the selection lines.
fn emit_keybind_args<A>(cmd: &mut Command, keybinds: &[MenuKeybind<A>]) {
    for bind in keybinds {
        cmd.arg("--bind")
            .arg(format!("{}:print({})+accept", bind.key, bind.key));
    }
}

/// Shared empty keybind list for menus without keybind support.
pub(crate) const NO_KEYBINDS: &[MenuKeybind<()>] = &[];

/// Reject duplicate keybind keys before they silently shadow each other.
fn validate_keybinds<A>(keybinds: &[MenuKeybind<A>]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for bind in keybinds {
        if !seen.insert(bind.key.as_str()) {
            bail!("duplicate menu keybind: {}", bind.key);
        }
    }
    Ok(())
}

/// Configure fzf for separator mode: raw mode + match-based navigation.
///
/// Best suited for short, static menus where visual grouping aids
/// discoverability. Avoid using separators in long, dynamically filtered
/// lists — raw mode keeps all items visible (dimmed when non-matching)
/// which can clutter large result sets.
fn configure_separator_mode(cmd: &mut Command) {
    cmd.arg("--raw");
    cmd.arg("--layout=reverse");
    cmd.arg(format!("--query={SELECTABLE_MARKER}"));
    cmd.arg("--gutter-raw= ");
    cmd.arg("--bind").arg(
        [
            "up:up-match",
            "down:down-match",
            "ctrl-p:up-match",
            "ctrl-n:down-match",
            "ctrl-k:up-match",
            "ctrl-j:down-match",
            "result:best",
        ]
        .join(","),
    );
}

/// Configure fzf preview and build input text based on the preview strategy.
/// Always includes the fzf_key in field 3 so we can reliably match items.
/// Search keywords are stored in field 2 for fuzzy matching.
///
/// NOTE: In practice, additional keywords only match if they are part of the
/// *visible* line. To keep them searchable without showing them, we append
/// "shadow keywords" to the display text after a large padding block so they
/// sit off-screen. This is intentionally hacky, but it is the only reliable
/// way to keep keyword matching working across fzf versions. The padding is
/// only applied when keywords exist.
/// Zero-width character used to mark selectable items in raw/separator mode.
/// Selectable items contain this in their display text so they match the
/// pre-set query, while separators do not and are therefore "non-matching"
/// (dimmed, skipped by up-match/down-match navigation).
const SELECTABLE_MARKER: &str = "\u{2060}";

struct FzfLineContext {
    separator_mode: bool,
}

fn format_fzf_line(
    display: &str,
    key: &str,
    keywords: &str,
    extra_fields: &[&str],
    ctx: &FzfLineContext,
    is_selectable: bool,
) -> String {
    // Shadow keywords: keep them in the visible line but push them off-screen.
    // Only apply the padding when keywords exist.
    const HIDDEN_PADDING: &str = "                                                                                                    ";

    let mut display_with_shadow = if keywords.is_empty() {
        display.to_string()
    } else {
        format!("{display}{HIDDEN_PADDING}{keywords}")
    };

    if ctx.separator_mode && is_selectable {
        display_with_shadow = format!("{SELECTABLE_MARKER}{display_with_shadow}");
    }

    let mut fields = Vec::with_capacity(3 + extra_fields.len());
    fields.push(display_with_shadow);
    fields.push(keywords.to_string());
    fields.push(key.to_string());
    for field in extra_fields {
        fields.push((*field).to_string());
    }
    fields.join("\x1f")
}

pub(crate) fn configure_preview_and_input(
    cmd: &mut Command,
    strategy: PreviewStrategy,
    display_data: &[ItemDisplayData],
    separator_mode: bool,
) -> String {
    // Check if any item has keywords
    let has_keywords = display_data.iter().any(|d| !d.keywords.is_empty());

    // Always hide extra fields (keywords, key, preview data) from display.
    // Field 1 remains the visible label (and contains any shadow keywords).
    cmd.arg("--delimiter=\x1f").arg("--with-nth=1");
    if has_keywords {
        cmd.arg("--no-hscroll");
    }

    let ctx = FzfLineContext { separator_mode };
    let fmt =
        |display_text: &str, key: &str, keywords: &str, is_selectable: bool, extra: &[&str]| {
            format_fzf_line(display_text, key, keywords, extra, &ctx, is_selectable)
        };

    match strategy {
        PreviewStrategy::None => display_data
            .iter()
            .map(|d| fmt(&d.display_text, &d.key, &d.keywords, d.is_selectable, &[]))
            .collect::<Vec<_>>()
            .join("\n"),
        PreviewStrategy::Command(command) => {
            let encoded = general_purpose::STANDARD.encode(command.as_bytes());
            cmd.arg("--preview").arg(format!(
                "key=$(echo {{}} | cut -d'\x1f' -f3); printf '%s' '{encoded}' | base64 -d | bash -s -- \"$key\""
            ));

            display_data
                .iter()
                .map(|d| fmt(&d.display_text, &d.key, &d.keywords, d.is_selectable, &[]))
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::CommandPerItem(command_map) => {
            cmd.arg("--preview")
                .arg("key=$(echo {} | cut -d'\x1f' -f3); echo {} | cut -d'\x1f' -f4 | base64 -d | bash -s -- \"$key\"");

            display_data
                .iter()
                .map(|d| {
                    let command = command_map.get(&d.key).cloned().unwrap_or_default();
                    let encoded = general_purpose::STANDARD.encode(command.as_bytes());
                    fmt(
                        &d.display_text,
                        &d.key,
                        &d.keywords,
                        d.is_selectable,
                        &[&encoded],
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::Text(preview_map) => {
            cmd.arg("--preview")
                .arg("echo {} | cut -d'\x1f' -f4 | base64 -d");

            display_data
                .iter()
                .map(|d| {
                    let preview = preview_map.get(&d.key).cloned().unwrap_or_default();
                    let encoded = general_purpose::STANDARD.encode(preview.as_bytes());
                    fmt(
                        &d.display_text,
                        &d.key,
                        &d.keywords,
                        d.is_selectable,
                        &[&encoded],
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::Mixed(mixed_map) => {
            cmd.arg("--preview").arg(
                "type=$(echo {} | cut -d'\x1f' -f4); content=$(echo {} | cut -d'\x1f' -f5 | base64 -d); \
                 key=$(echo {} | cut -d'\x1f' -f3); \
                 if [ \"$type\" = 'C' ]; then echo \"$content\" | bash -s -- \"$key\"; else echo \"$content\"; fi",
            );

            display_data
                .iter()
                .map(|d| {
                    let (type_marker, content) = match mixed_map.get(&d.key) {
                        Some(MixedPreviewContent::Text(text)) => ("T", text.clone()),
                        Some(MixedPreviewContent::Command(cmd)) => ("C", cmd.clone()),
                        None => ("T", String::new()),
                    };
                    let encoded = general_purpose::STANDARD.encode(content.as_bytes());
                    fmt(
                        &d.display_text,
                        &d.key,
                        &d.keywords,
                        d.is_selectable,
                        &[type_marker, &encoded],
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

/// Apply the standard menu arguments shared by `select` and
/// `select_streaming`.
fn configure_menu_args<A>(cmd: &mut Command, wrapper: &FzfWrapper, keybinds: &[MenuKeybind<A>]) {
    cmd.arg("--ansi"); // Enable ANSI color interpretation in display text
    cmd.arg("--tiebreak=index");

    if wrapper.multi_select {
        cmd.arg("--multi");
    }
    if let Some(prompt) = &wrapper.prompt {
        cmd.arg("--prompt").arg(format!("{prompt} > "));
    }
    let header_text = match &wrapper.header {
        Some(header) => {
            let mut text = header.clone();
            if !keybinds.is_empty() {
                text.push('\n');
                text.push_str(&keybind_hint_line(keybinds));
            }
            Some(text)
        }
        None if !keybinds.is_empty() => Some(keybind_hint_line(keybinds)),
        None => None,
    };
    if let Some(header_text) = header_text {
        cmd.arg("--header").arg(header_text);
    }
    emit_keybind_args(cmd, keybinds);
}

/// Force every item's preview through the `Mixed` strategy so items
/// streamed in later can carry either text or command previews.
fn force_mixed_preview_strategy<T: FzfSelectable>(items: &[T]) -> PreviewStrategy {
    PreviewUtils::force_mixed_preview_strategy(items)
}

/// Spawn fzf with piped stdio and register it with the menu server.
fn spawn_menu_child(mut cmd: Command) -> Result<(std::process::Child, u32)> {
    let child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let pid = child.id();
    let _ = crate::menu::server::register_menu_process(pid);
    Ok((child, pid))
}

/// Wait for fzf to exit, unregister it, and map wait failures through the
/// standard fzf error handling.
fn finish_menu_child(child: std::process::Child, pid: u32) -> Result<std::process::Output> {
    let output = child.wait_with_output();
    crate::menu::server::unregister_menu_process(pid);

    match output {
        Ok(result) => Ok(result),
        Err(e) => {
            super::utils::handle_fzf_spawn_error(&e);
            Err(anyhow!("fzf execution failed: {e}"))
        }
    }
}

/// Execute the fzf command with the given input and return the raw output.
fn execute_fzf_command(cmd: Command, input_text: &str) -> Result<std::process::Output> {
    let (mut child, pid) = spawn_menu_child(cmd)?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input_text.as_bytes())?;
    }

    finish_menu_child(child, pid)
}

/// Map a pressed keybind token back to the caller's typed action.
fn resolve_action_token<A: Clone>(token: &str, keybinds: &[MenuKeybind<A>]) -> Result<A> {
    keybinds
        .iter()
        .find(|bind| bind.key.as_str() == token)
        .map(|bind| bind.action.clone())
        .ok_or_else(|| anyhow!("fzf returned unknown keybind token {token:?}"))
}

/// Parse fzf output into submitted items plus an optional pressed keybind.
///
/// Selection lines always carry at least three `\x1f`-delimited fields; a
/// leading line exactly equal to one of the registered keys is the token
/// printed by `--bind '{key}:print({key})+accept'`. A token with no item
/// lines means the bind was pressed while the filtered list was empty.
fn parse_fzf_output<T: Clone, A: Clone>(
    result: std::process::Output,
    item_map: &HashMap<String, T>,
    keybinds: &[MenuKeybind<A>],
) -> Result<DialogOutcome<MenuSelection<T, A>>> {
    if fzf_was_cancelled(&result)? {
        return Ok(DialogOutcome::Cancelled);
    }

    let stdout = String::from_utf8_lossy(&result.stdout);
    let mut lines = stdout
        .trim_end()
        .split('\n')
        .filter(|line| !line.is_empty())
        .peekable();

    let action_token = match lines.peek() {
        Some(line) if keybinds.iter().any(|bind| bind.key.as_str() == *line) => {
            Some((*line).to_string())
        }
        _ => None,
    };
    if action_token.is_some() {
        lines.next();
    }

    // A bare line without any \x1f separator cannot be a selection line.
    // With binds registered it can only be a token we did not register (fzf
    // never emits one on its own); reject it explicitly instead of reporting
    // a missing item key. Without binds, a bare line is malformed selection
    // input, so leave it for the item-key check below.
    if !keybinds.is_empty() && lines.peek().is_some_and(|line| !line.contains('\x1f')) {
        let token = lines.next().unwrap_or_default();
        anyhow::bail!("fzf returned unknown keybind token {token:?}");
    }

    // Extract the key from field 3 (format: display\x1fkeywords\x1fkey[\x1f...])
    let items = lines
        .map(|line| {
            let key = line
                .split('\x1f')
                .nth(2)
                .ok_or_else(|| anyhow!("fzf returned a selection without an item key"))?;
            item_map
                .get(key)
                .cloned()
                .ok_or_else(|| anyhow!("fzf returned unknown item key {key:?}"))
        })
        .collect::<Result<Vec<_>>>()?;

    let action = match &action_token {
        Some(token) => Some(resolve_action_token(token, keybinds)?),
        None => None,
    };

    if action_token.is_none() && items.is_empty() {
        return Ok(DialogOutcome::Cancelled);
    }

    Ok(DialogOutcome::Submitted(MenuSelection { items, action }))
}

fn parse_encoded_streaming_output<T: DeserializeOwned>(
    result: std::process::Output,
) -> Result<DialogOutcome<MenuSelection<DecodedStreamingMenuItem<T>>>> {
    if fzf_was_cancelled(&result)? {
        return Ok(DialogOutcome::Cancelled);
    }

    let stdout = String::from_utf8_lossy(&result.stdout);
    let selected_lines: Vec<&str> = stdout
        .trim_end()
        .split('\n')
        .filter(|line| !line.is_empty())
        .collect();

    if selected_lines.is_empty() {
        return Ok(DialogOutcome::Cancelled);
    }

    let items = selected_lines
        .into_iter()
        .map(DecodedStreamingMenuItem::decode)
        .collect::<Result<Vec<_>>>()?;
    Ok(DialogOutcome::Submitted(MenuSelection::from_items(items)))
}

pub struct FzfWrapper {
    pub(crate) multi_select: bool,
    pub(crate) prompt: Option<String>,
    pub(crate) header: Option<String>,
    pub(crate) additional_args: Vec<String>,
    pub(crate) initial_cursor: Option<InitialCursor>,
    pub(crate) responsive_layout: bool,
}

impl FzfWrapper {
    pub fn builder() -> super::builder::FzfBuilder {
        super::builder::FzfBuilder::new()
    }

    /// Build a selection menu with the standard instantCLI theme, search
    /// prompt, and responsive preview layout.
    pub fn menu() -> super::builder::FzfBuilder {
        Self::builder()
            .prompt(format!("{} ", char::from(NerdFont::Search)))
            // The empty default header intentionally renders as vertical
            // breathing room between the query line and the first row.
            .header(Header::default(""))
            .responsive_layout()
    }

    pub(crate) fn from_builder(b: super::builder::FzfBuilder) -> Self {
        let parts = b.into_wrapper_parts();
        Self {
            multi_select: parts.multi_select,
            prompt: parts.prompt,
            header: parts.header.map(|h| h.to_fzf_string()),
            additional_args: parts.additional_args,
            initial_cursor: parts.initial_cursor,
            responsive_layout: parts.responsive_layout,
        }
    }

    /// Selection without keybinds. See [`FzfWrapper::select_with_keybinds`].
    pub fn select<T: FzfSelectable + Clone>(
        &self,
        items: Vec<T>,
    ) -> Result<DialogOutcome<MenuSelection<T>>> {
        self.select_with_keybinds(items, NO_KEYBINDS)
    }

    /// Selection with globally registered keybinds.
    ///
    /// Each bind terminates fzf when pressed and returns its typed action
    /// alongside the current selection set (empty when the filtered list was
    /// empty). Unlike [`FzfWrapper::select`], an empty item list is still
    /// shown so keybinds remain usable; a menu without keybinds and without
    /// items stays a plain cancellation.
    pub fn select_with_keybinds<T: FzfSelectable + Clone, A: Clone>(
        &self,
        items: Vec<T>,
        keybinds: &[MenuKeybind<A>],
    ) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return Ok(crate::menu_utils::mock::resolve_selection(
                resp, items, keybinds,
            ));
        }
        validate_keybinds(keybinds)?;
        if items.is_empty() && keybinds.is_empty() {
            return Ok(DialogOutcome::Cancelled);
        }

        // Build item lookup map (keyed by fzf_key) and display data with search keywords
        let (item_map, display_data) = build_item_map(&items);

        // Detect separator mode: any non-selectable items present
        let separator_mode = display_data.iter().any(|d| !d.is_selectable);

        // Calculate initial cursor position, adjusting for separators
        let cursor_position = if separator_mode {
            calculate_separator_aware_cursor(&self.initial_cursor, &display_data)
        } else {
            calculate_cursor_position(&self.initial_cursor, display_data.len())
        };

        // Analyze preview strategy and build input text
        let preview_strategy = PreviewUtils::analyze_preview_strategy(&items)?;

        // Configure fzf command
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        configure_menu_args(&mut cmd, self, keybinds);

        // Build input text and configure preview
        let input_text =
            configure_preview_and_input(&mut cmd, preview_strategy, &display_data, separator_mode);

        if let Some(position) = cursor_position {
            cmd.arg("--bind").arg(format!("load:pos({})", position + 1));
        }
        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        // Enable raw mode with separator-skipping navigation
        if separator_mode {
            configure_separator_mode(&mut cmd);
        }

        // Apply responsive layout settings LAST to override defaults
        if self.responsive_layout {
            let layout = super::utils::get_responsive_layout();
            cmd.arg(layout.preview_window);
            cmd.arg("--margin").arg(layout.margin);
        }

        // Execute fzf
        let output = execute_fzf_command(cmd, &input_text)?;

        // Parse output and map back to items
        parse_fzf_output(output, &item_map, keybinds)
    }

    pub fn select_encoded_streaming_prefilled<T, C>(
        &self,
        producer: C,
        initial_input: &str,
    ) -> Result<DialogOutcome<MenuSelection<DecodedStreamingMenuItem<T>>>>
    where
        T: DeserializeOwned,
        C: Into<StreamingCommand>,
    {
        let output = self.execute_streaming_command(
            producer,
            initial_input,
            &[
                "--delimiter",
                "\t",
                "--with-nth",
                "3",
                "--preview",
                streaming_preview_command(),
                "--ansi",
            ],
        )?;
        parse_encoded_streaming_output(output)
    }

    pub fn select_encoded_streaming<T, C>(
        &self,
        producer: C,
    ) -> Result<DialogOutcome<MenuSelection<DecodedStreamingMenuItem<T>>>>
    where
        T: DeserializeOwned,
        C: Into<StreamingCommand>,
    {
        self.select_encoded_streaming_prefilled(producer, "")
    }

    fn execute_streaming_command<C>(
        &self,
        producer: C,
        initial_input: &str,
        base_args: &[&str],
    ) -> Result<std::process::Output>
    where
        C: Into<StreamingCommand>,
    {
        let mut fzf = Command::new("fzf");
        fzf.env_remove("FZF_DEFAULT_OPTS");
        for arg in self.streaming_fzf_args(base_args) {
            fzf.arg(arg);
        }

        let (mut fzf_child, pid) = spawn_menu_child(fzf)?;

        let mut producer = producer.into().into_command();
        producer
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut producer_child = producer.spawn()?;
        let mut producer_stdout = producer_child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture streaming producer stdout"))?;
        let mut fzf_stdin = fzf_child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to capture fzf stdin"))?;
        let initial = initial_input.to_string();

        let pump = thread::spawn(move || -> Result<()> {
            if !initial.is_empty() {
                fzf_stdin.write_all(initial.as_bytes())?;
                if !initial.ends_with('\n') {
                    fzf_stdin.write_all(b"\n")?;
                }
                fzf_stdin.flush()?;
            }

            let mut reader = BufReader::new(&mut producer_stdout);
            let mut line = String::new();
            loop {
                line.clear();
                let bytes = reader.read_line(&mut line)?;
                if bytes == 0 {
                    break;
                }

                if fzf_stdin.write_all(line.as_bytes()).is_err() {
                    break;
                }
            }

            Ok(())
        });

        let result = finish_menu_child(fzf_child, pid);
        let _ = producer_child.kill();
        let _ = producer_child.wait();
        let _ = pump.join();
        result
    }

    fn streaming_fzf_args(&self, base_args: &[&str]) -> Vec<String> {
        let mut fzf_args = vec!["--tiebreak=index".to_string()];
        fzf_args.extend(base_args.iter().map(|arg| (*arg).to_string()));

        if self.multi_select {
            fzf_args.push("--multi".to_string());
        }

        if let Some(prompt) = &self.prompt {
            fzf_args.push("--prompt".to_string());
            fzf_args.push(format!("{} > ", prompt));
        }

        if let Some(header) = &self.header {
            fzf_args.push("--header".to_string());
            fzf_args.push(header.clone());
        }

        fzf_args.extend(self.additional_args.clone());

        if let Some(InitialCursor::Index(idx)) = self.initial_cursor {
            fzf_args.push("--bind".to_string());
            fzf_args.push(format!("load:pos({})", idx + 1));
        }

        if self.responsive_layout {
            let layout = super::utils::get_responsive_layout();
            fzf_args.push(layout.preview_window.to_string());
            fzf_args.push("--margin".to_string());
            fzf_args.push(layout.margin.to_string());
        }

        fzf_args
    }

    /// Like [`FzfWrapper::select`], but further items stream in while the
    /// menu is open.
    ///
    /// `initial_items` are shown immediately; additional items are pulled
    /// from `late_items` and appended live until the channel closes. The
    /// preview strategy is forced to `Mixed` because the preview kind of
    /// late items is not known when fzf is spawned.
    pub fn select_streaming<T: FzfSelectable + Clone + Send + 'static>(
        &self,
        initial_items: Vec<T>,
        late_items: Receiver<T>,
    ) -> Result<DialogOutcome<MenuSelection<T>>> {
        self.select_streaming_with_ready(initial_items, late_items, || Ok(()))
    }

    /// Streaming selection with a callback fired after fzf has spawned and
    /// its initial input has been flushed. Servers use this as the readiness
    /// boundary before allowing producers to consume their input.
    pub fn select_streaming_with_ready<
        T: FzfSelectable + Clone + Send + 'static,
        F: FnOnce() -> Result<()>,
    >(
        &self,
        initial_items: Vec<T>,
        late_items: Receiver<T>,
        on_ready: F,
    ) -> Result<DialogOutcome<MenuSelection<T>>> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            on_ready()?;
            let mut items = initial_items;
            while let Ok(item) = late_items.try_recv() {
                items.push(item);
            }
            return Ok(crate::menu_utils::mock::resolve_selection(
                resp,
                items,
                NO_KEYBINDS,
            ));
        }

        let (item_map, display_data) = build_item_map(&initial_items);
        let separator_mode = display_data.iter().any(|d| !d.is_selectable);
        let preview_strategy = force_mixed_preview_strategy(&initial_items);

        // Configure fzf command
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        configure_menu_args(&mut cmd, self, NO_KEYBINDS);
        let input_text =
            configure_preview_and_input(&mut cmd, preview_strategy, &display_data, separator_mode);

        for arg in &self.additional_args {
            cmd.arg(arg);
        }
        if separator_mode {
            configure_separator_mode(&mut cmd);
        }
        if self.responsive_layout {
            let layout = super::utils::get_responsive_layout();
            cmd.arg(layout.preview_window);
            cmd.arg("--margin").arg(layout.margin);
        }

        let (mut child, pid) = spawn_menu_child(cmd)?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to capture fzf stdin"))?;
        // `input_text` is newline-joined without a trailing newline, but the
        // pump below appends complete lines. Terminate the initial block so
        // the first streamed item cannot glue onto the last initial row.
        if !input_text.is_empty() {
            stdin.write_all(input_text.as_bytes())?;
            if !input_text.ends_with('\n') {
                stdin.write_all(b"\n")?;
            }
            stdin.flush()?;
        }
        if let Err(error) = on_ready() {
            let _ = child.kill();
            let _ = finish_menu_child(child, pid);
            return Err(error);
        }

        // Late items are appended to fzf's stdin as they arrive; the shared
        // map lets the final selection be resolved back to an item.
        let shared_map = Arc::new(Mutex::new(item_map));
        let pump_map = Arc::clone(&shared_map);
        let (cancel_tx, cancel_rx) = bounded::<()>(1);
        let pump = thread::spawn(move || {
            let mut stdin = stdin;
            loop {
                let first = select! {
                    recv(cancel_rx) -> _ => break,
                    recv(late_items) -> item => match item {
                        Ok(item) => item,
                        Err(_) => break,
                    },
                };

                let Ok(mut map) = pump_map.lock() else {
                    break;
                };
                let mut batch = Vec::with_capacity(64);
                batch.push(first);
                while batch.len() < 64 {
                    match late_items.try_recv() {
                        Ok(item) => batch.push(item),
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }

                let mut encoded_batch = String::new();
                for item in batch {
                    let key = item.fzf_key();
                    if map.contains_key(&key) {
                        continue;
                    }
                    let (marker, content) = match item.fzf_preview() {
                        FzfPreview::Text(text) => ("T", text),
                        FzfPreview::Command(command) => ("C", command),
                        FzfPreview::None => ("T", String::new()),
                    };
                    let encoded = general_purpose::STANDARD.encode(content.as_bytes());
                    let line = format_fzf_line(
                        &item.fzf_display_text(),
                        &key,
                        &item.fzf_search_keywords().join(" "),
                        &[marker, &encoded],
                        &FzfLineContext { separator_mode },
                        item.fzf_is_selectable(),
                    );
                    encoded_batch.push_str(&line);
                    encoded_batch.push('\n');
                    map.insert(key, item);
                }

                if (!encoded_batch.is_empty()
                    && stdin
                        .write_all(encoded_batch.as_bytes())
                        .and_then(|_| stdin.flush())
                        .is_err())
                    || cancel_rx.try_recv().is_ok()
                {
                    break;
                }
            }
        });

        let output = finish_menu_child(child, pid);
        let _ = cancel_tx.try_send(());
        let _ = pump.join();
        let output = output?;

        let item_map = shared_map
            .lock()
            .map_err(|_| anyhow!("fzf streaming item map poisoned"))?;
        parse_fzf_output(output, &item_map, NO_KEYBINDS)
    }

    pub fn input(prompt: &str) -> Result<DialogOutcome<String>> {
        Self::builder().prompt(prompt).input().input_dialog()
    }

    pub fn message(message: &str) -> Result<()> {
        Self::builder().message(message).message_dialog()
    }

    pub fn confirm(message: &str) -> Result<ConfirmResult> {
        Self::builder().confirm(message).confirm_dialog()
    }

    pub fn password(prompt: &str) -> Result<DialogOutcome<String>> {
        Self::builder().prompt(prompt).password().password_dialog()
    }
}

#[cfg(test)]
mod mock_tests {
    use super::*;
    use crate::menu_utils::MockQueue;

    #[test]
    fn test_mock_select_returns_canned_item() {
        let _guard = MockQueue::new().select_index(1).guard();
        let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let result = FzfWrapper::builder().select(items).unwrap();
        match result {
            DialogOutcome::Submitted(sel) => assert_eq!(sel.items, vec!["beta".to_string()]),
            other => panic!("Expected Submitted, got {other:?}"),
        }
    }

    #[test]
    fn test_mock_select_cancel() {
        let _guard = MockQueue::new().cancel_selection().guard();
        let items = vec!["alpha".to_string()];
        let result = FzfWrapper::builder().select(items).unwrap();
        assert_eq!(result, DialogOutcome::Cancelled);
    }

    #[test]
    fn test_mock_multi_select_returns_all_canned_items() {
        let _guard = MockQueue::new().multi_select(vec![0, 2]).guard();
        let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let result = FzfWrapper::builder()
            .multi_select(true)
            .select(items)
            .unwrap();
        assert_eq!(
            result,
            DialogOutcome::Submitted(MenuSelection {
                items: vec!["alpha".to_string(), "gamma".to_string()],
                action: None,
            })
        );
    }

    #[test]
    fn select_one_preserves_submission_and_cancellation() {
        let _guard = MockQueue::new().select_index(0).cancel_selection().guard();
        let submitted = FzfWrapper::builder()
            .select_one(vec!["alpha".to_string()])
            .unwrap();
        let cancelled = FzfWrapper::builder()
            .select_one(vec!["alpha".to_string()])
            .unwrap();

        assert_eq!(submitted, DialogOutcome::Submitted("alpha".to_string()));
        assert_eq!(cancelled, DialogOutcome::Cancelled);
    }

    #[test]
    fn select_one_rejects_multi_selection_results() {
        let _guard = MockQueue::new().multi_select(vec![0]).guard();
        let error = FzfWrapper::builder()
            .multi_select(true)
            .select_one(vec!["alpha".to_string()])
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "select_one cannot be used with multi-selection enabled"
        );
    }

    #[test]
    fn select_menu_rejects_multi_selection_results() {
        use crate::menu_utils::MenuItem;

        let _guard = MockQueue::new().multi_select(vec![0, 1]).guard();
        let entries = vec![
            MenuItem::entry("alpha".to_string()),
            MenuItem::entry("beta".to_string()),
        ];
        let error = FzfWrapper::builder().select_menu(entries).unwrap_err();

        assert_eq!(
            error.to_string(),
            "expected exactly one selected menu entry, got 2"
        );
    }

    #[test]
    fn fzf_status_distinguishes_cancellation_from_failure() {
        use std::os::unix::process::ExitStatusExt;

        let output = |code| std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: b"renderer failed".to_vec(),
        };

        assert!(!fzf_was_cancelled(&output(0)).unwrap());
        assert!(!fzf_was_cancelled(&output(1)).unwrap());
        assert!(fzf_was_cancelled(&output(130)).unwrap());
        assert!(fzf_was_cancelled(&output(2)).is_err());
    }

    #[test]
    fn malformed_fzf_selection_is_an_error() {
        use std::os::unix::process::ExitStatusExt;

        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"label without fields\n".to_vec(),
            stderr: Vec::new(),
        };
        let items = HashMap::from([("known".to_string(), "value".to_string())]);

        let error = parse_fzf_output(output, &items, NO_KEYBINDS).unwrap_err();
        assert_eq!(
            error.to_string(),
            "fzf returned a selection without an item key"
        );
    }

    #[test]
    fn keybind_token_is_peeled_and_mapped_to_action() {
        use std::os::unix::process::ExitStatusExt;

        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"ctrl-e\nlabel\x1f\x1fknown\n".to_vec(),
            stderr: Vec::new(),
        };
        let items = HashMap::from([("known".to_string(), "value".to_string())]);
        let keybinds = [MenuKeybind::new(
            MenuKey::new("ctrl-e").unwrap(),
            "edit",
            7u8,
        )];

        let outcome = parse_fzf_output(output, &items, &keybinds).unwrap();
        let selection = match outcome {
            DialogOutcome::Submitted(sel) => sel,
            other => panic!("Expected Submitted, got {other:?}"),
        };
        assert_eq!(selection.items, vec!["value"]);
        assert_eq!(selection.action, Some(7u8));
    }

    #[test]
    fn keybind_press_on_empty_list_yields_action_without_items() {
        use std::os::unix::process::ExitStatusExt;

        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"ctrl-e\n".to_vec(),
            stderr: Vec::new(),
        };
        let items = HashMap::from([("known".to_string(), "value".to_string())]);
        let keybinds = [MenuKeybind::new(
            MenuKey::new("ctrl-e").unwrap(),
            "edit",
            1u8,
        )];

        let outcome = parse_fzf_output(output, &items, &keybinds).unwrap();
        let selection = match outcome {
            DialogOutcome::Submitted(sel) => sel,
            other => panic!("Expected Submitted, got {other:?}"),
        };
        assert!(selection.items.is_empty());
        assert_eq!(selection.action, Some(1u8));
    }

    #[test]
    fn enter_submission_has_no_action() {
        use std::os::unix::process::ExitStatusExt;

        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"label\x1f\x1fknown\n".to_vec(),
            stderr: Vec::new(),
        };
        let items = HashMap::from([("known".to_string(), "value".to_string())]);
        let keybinds = [MenuKeybind::new(
            MenuKey::new("ctrl-e").unwrap(),
            "edit",
            1u8,
        )];

        let outcome = parse_fzf_output(output, &items, &keybinds).unwrap();
        let selection = match outcome {
            DialogOutcome::Submitted(sel) => sel,
            other => panic!("Expected Submitted, got {other:?}"),
        };
        assert_eq!(selection.items, vec!["value"]);
        assert_eq!(selection.action, None);
    }

    #[test]
    fn unknown_keybind_token_is_an_error() {
        use std::os::unix::process::ExitStatusExt;

        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"ctrl-z\nlabel\x1f\x1fknown\n".to_vec(),
            stderr: Vec::new(),
        };
        let items = HashMap::from([("known".to_string(), "value".to_string())]);
        let keybinds = [MenuKeybind::new(
            MenuKey::new("ctrl-e").unwrap(),
            "edit",
            1u8,
        )];

        let error = parse_fzf_output(output, &items, &keybinds).unwrap_err();
        assert_eq!(
            error.to_string(),
            "fzf returned unknown keybind token \"ctrl-z\""
        );
    }

    #[test]
    fn menu_key_rejects_malformed_and_reserved_names() {
        assert!(MenuKey::new("").is_err());
        assert!(MenuKey::new("Ctrl-E").is_err());
        assert!(MenuKey::new("ctrl_e").is_err());
        assert!(MenuKey::new("ctrl e").is_err());
        assert!(MenuKey::new("ctrl-e").is_ok());
        assert!(MenuKey::new("f3").is_ok());
        assert!(MenuKey::new("alt-s").is_ok());
        for reserved in [
            "esc", "ctrl-c", "enter", "tab", "ctrl-p", "ctrl-j", "up", "down",
        ] {
            assert!(
                MenuKey::new(reserved).is_err(),
                "{reserved} should be reserved"
            );
        }
    }

    #[test]
    fn duplicate_keybinds_are_rejected() {
        let binds = [
            MenuKeybind::new(MenuKey::new("ctrl-e").unwrap(), "a", 1u8),
            MenuKeybind::new(MenuKey::new("ctrl-e").unwrap(), "b", 2u8),
        ];
        let error = validate_keybinds(&binds).unwrap_err();
        assert_eq!(error.to_string(), "duplicate menu keybind: ctrl-e");
    }

    #[test]
    fn test_mock_select_streaming_merges_streamed_items() {
        let (tx, rx) = crossbeam_channel::unbounded::<String>();
        tx.send("late".to_string()).unwrap();
        let _tx = tx; // keep the channel open, mirroring a live producer

        let _guard = MockQueue::new().select_index(1).guard();
        let result = FzfWrapper::builder()
            .select_streaming(vec!["static".to_string()], rx)
            .unwrap();
        match result {
            DialogOutcome::Submitted(sel) => assert_eq!(sel.items, vec!["late".to_string()]),
            other => panic!("Expected Submitted, got {other:?}"),
        }
    }

    #[test]
    fn standard_menu_keeps_spacing_header() {
        let parts = FzfWrapper::menu().into_wrapper_parts();
        assert!(matches!(parts.header, Some(Header::Default(text)) if text.is_empty()));
    }
}
