//! FZF wrapper and selection logic

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::common::shell::shell_quote;

use super::preview::MixedPreviewContent;
use super::preview::PreviewStrategy;
use super::preview::PreviewUtils;
use super::types::*;
use super::utils::{check_for_old_fzf_and_exit, log_fzf_failure};

// ============================================================================
// Helper functions for FzfWrapper::select
// ============================================================================

/// Item data collected for fzf display: (display_text, key, search_keywords, is_selectable)
type ItemDisplayData = (String, String, String, bool);

/// Build a lookup map from fzf_key to item, and collect display lines with keys and search keywords.
/// Non-selectable items (separators) are included in display_data but excluded from item_map.
fn build_item_map<T: FzfSelectable + Clone>(
    items: &[T],
) -> (HashMap<String, T>, Vec<ItemDisplayData>) {
    let mut item_map: HashMap<String, T> = HashMap::new();
    let mut display_data = Vec::new();

    for item in items {
        let display = item.fzf_display_text();
        let key = item.fzf_key();
        let keywords = item.fzf_search_keywords().join(" ");
        let selectable = item.fzf_is_selectable();
        display_data.push((display, key.clone(), keywords, selectable));
        if selectable {
            item_map.insert(key, item.clone());
        }
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
    if display_data[pos].3 {
        return Some(pos);
    }

    // Search forward for the nearest selectable item
    if let Some(fwd) = display_data[pos..].iter().position(|d| d.3) {
        return Some(pos + fwd);
    }

    // Search backward
    display_data[..pos].iter().rposition(|d| d.3)
}

/// Configure fzf for separator mode: raw mode + match-based navigation.
fn configure_separator_mode(cmd: &mut Command) {
    cmd.arg("--raw");
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

fn format_fzf_line(
    display: &str,
    key: &str,
    keywords: &str,
    extra_fields: &[&str],
    separator_mode: bool,
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

    if separator_mode && is_selectable {
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
    let has_keywords = display_data
        .iter()
        .any(|(_, _, keywords, _)| !keywords.is_empty());

    // Always hide extra fields (keywords, key, preview data) from display.
    // Field 1 remains the visible label (and contains any shadow keywords).
    cmd.arg("--delimiter=\x1f").arg("--with-nth=1");
    if has_keywords {
        cmd.arg("--no-hscroll");
    }

    let fmt = |display: &str, key: &str, keywords: &str, selectable: bool, extra: &[&str]| {
        format_fzf_line(display, key, keywords, extra, separator_mode, selectable)
    };

    match strategy {
        PreviewStrategy::None => display_data
            .iter()
            .map(|(display, key, keywords, sel)| fmt(display, key, keywords, *sel, &[]))
            .collect::<Vec<_>>()
            .join("\n"),
        PreviewStrategy::Command(command) => {
            let encoded = general_purpose::STANDARD.encode(command.as_bytes());
            cmd.arg("--preview").arg(format!(
                "key=$(echo {{}} | cut -d'\x1f' -f3); printf '%s' '{encoded}' | base64 -d | bash -s -- \"$key\""
            ));

            display_data
                .iter()
                .map(|(display, key, keywords, sel)| fmt(display, key, keywords, *sel, &[]))
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::CommandPerItem(command_map) => {
            cmd.arg("--preview")
                .arg("key=$(echo {} | cut -d'\x1f' -f3); echo {} | cut -d'\x1f' -f4 | base64 -d | bash -s -- \"$key\"");

            display_data
                .iter()
                .map(|(display, key, keywords, sel)| {
                    let command = command_map.get(display).cloned().unwrap_or_default();
                    let encoded = general_purpose::STANDARD.encode(command.as_bytes());
                    fmt(display, key, keywords, *sel, &[&encoded])
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::Text(preview_map) => {
            cmd.arg("--preview")
                .arg("echo {} | cut -d'\x1f' -f4 | base64 -d");

            display_data
                .iter()
                .map(|(display, key, keywords, sel)| {
                    let preview = preview_map.get(display).cloned().unwrap_or_default();
                    let encoded = general_purpose::STANDARD.encode(preview.as_bytes());
                    fmt(display, key, keywords, *sel, &[&encoded])
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
                .map(|(display, key, keywords, sel)| {
                    let (type_marker, content) = match mixed_map.get(display) {
                        Some(MixedPreviewContent::Text(text)) => ("T", text.clone()),
                        Some(MixedPreviewContent::Command(cmd)) => ("C", cmd.clone()),
                        None => ("T", String::new()),
                    };
                    let encoded = general_purpose::STANDARD.encode(content.as_bytes());
                    fmt(display, key, keywords, *sel, &[type_marker, &encoded])
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

/// Execute the fzf command with the given input and return the raw output.
fn execute_fzf_command(mut cmd: Command, input_text: &str) -> Result<std::process::Output> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let pid = child.id();
    let _ = crate::menu::server::register_menu_process(pid);

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input_text.as_bytes())?;
    }

    let output = child.wait_with_output();
    crate::menu::server::unregister_menu_process(pid);

    output.map_err(Into::into)
}

/// Parse fzf output and map selected lines back to items using the fzf_key field.
fn parse_fzf_output<T: Clone>(
    result: std::process::Output,
    item_map: &HashMap<String, T>,
    multi_select: bool,
) -> Result<FzfResult<T>> {
    // Handle cancellation (Esc, Ctrl-C)
    if let Some(code) = result.status.code()
        && (code == 130 || code == 143)
    {
        return Ok(FzfResult::Cancelled);
    }

    // Log failures for debugging
    if !result.status.success() {
        check_for_old_fzf_and_exit(&result.stderr);
        if crate::ui::is_debug_enabled() {
            log_fzf_failure(&result.stderr, result.status.code(), |code, message| {
                crate::ui::emit(crate::ui::Level::Debug, code, message, None);
            });
        }
    }

    // Parse selected lines
    let stdout = String::from_utf8_lossy(&result.stdout);
    let selected_lines: Vec<&str> = stdout
        .trim_end()
        .split('\n')
        .filter(|line| !line.is_empty())
        .collect();

    if selected_lines.is_empty() {
        return Ok(FzfResult::Cancelled);
    }

    // Extract the key from field 3 (format: display\x1fkeywords\x1fkey[\x1f...])
    if multi_select {
        let selected_items: Vec<T> = selected_lines
            .iter()
            .filter_map(|line| {
                // Extract key from field 3 (index 2) using Unit Separator
                let key = line.split('\x1f').nth(2)?;
                item_map.get(key).cloned()
            })
            .collect();
        Ok(FzfResult::MultiSelected(selected_items))
    } else {
        // Extract key from field 3 (index 2) using Unit Separator
        if let Some(key) = selected_lines[0].split('\x1f').nth(2) {
            match item_map.get(key).cloned() {
                Some(item) => Ok(FzfResult::Selected(item)),
                None => Ok(FzfResult::Cancelled),
            }
        } else {
            Ok(FzfResult::Cancelled)
        }
    }
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

    pub(crate) fn new(
        multi_select: bool,
        prompt: Option<String>,
        header: Option<Header>,
        additional_args: Vec<String>,
        initial_cursor: Option<InitialCursor>,
        responsive_layout: bool,
    ) -> Self {
        Self {
            multi_select,
            prompt,
            header: header.map(|h| h.to_fzf_string()),
            additional_args,
            initial_cursor,
            responsive_layout,
        }
    }

    pub fn select_streaming(&self, input_command: &str) -> Result<FzfResult<String>> {
        let mut fzf_args = vec!["--tiebreak=index".to_string()];

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

        // Apply responsive layout settings LAST to override defaults
        if self.responsive_layout {
            let layout = super::utils::get_responsive_layout();
            fzf_args.push(layout.preview_window.to_string());
            fzf_args.push("--margin".to_string());
            fzf_args.push(layout.margin.to_string());
        }

        let mut cmd = Command::new("sh");
        cmd.arg("-c");

        let escaped_args: Vec<String> = fzf_args.iter().map(|arg| shell_quote(arg)).collect();
        let fzf_cmd = format!("fzf {}", escaped_args.join(" "));
        let full_command = format!("unset FZF_DEFAULT_OPTS; {} | {}", input_command, fzf_cmd);

        cmd.arg(&full_command);

        let child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        let output = child.wait_with_output();
        crate::menu::server::unregister_menu_process(pid);

        match output {
            Ok(result) => {
                if let Some(code) = result.status.code()
                    && (code == 130 || code == 143)
                {
                    return Ok(FzfResult::Cancelled);
                }

                if !result.status.success() {
                    check_for_old_fzf_and_exit(&result.stderr);
                    if crate::ui::is_debug_enabled() {
                        log_fzf_failure(&result.stderr, result.status.code(), |code, message| {
                            crate::ui::emit(crate::ui::Level::Debug, code, message, None);
                        });
                    }
                }

                let stdout = String::from_utf8_lossy(&result.stdout);
                let selected_lines: Vec<&str> = stdout
                    .trim_end()
                    .split('\n')
                    .filter(|line| !line.is_empty())
                    .collect();

                if selected_lines.is_empty() {
                    Ok(FzfResult::Cancelled)
                } else if self.multi_select {
                    let items: Vec<String> = selected_lines.iter().map(|s| s.to_string()).collect();
                    Ok(FzfResult::MultiSelected(items))
                } else {
                    Ok(FzfResult::Selected(selected_lines[0].to_string()))
                }
            }
            Err(e) => Ok(FzfResult::Error(format!("fzf execution failed: {e}"))),
        }
    }

    pub fn select<T: FzfSelectable + Clone>(&self, items: Vec<T>) -> Result<FzfResult<T>> {
        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
        }

        // Build item lookup map (keyed by fzf_key) and display data with search keywords
        let (item_map, display_data) = build_item_map(&items);

        // Detect separator mode: any non-selectable items present
        let separator_mode = display_data.iter().any(|(_, _, _, sel)| !sel);

        // Calculate initial cursor position, adjusting for separators
        let cursor_position = if separator_mode {
            calculate_separator_aware_cursor(
                &self.initial_cursor,
                &display_data,
            )
        } else {
            calculate_cursor_position(&self.initial_cursor, display_data.len())
        };

        // Analyze preview strategy and build input text
        let preview_strategy = PreviewUtils::analyze_preview_strategy(&items)?;

        // Configure fzf command
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--ansi"); // Enable ANSI color interpretation in display text
        cmd.arg("--tiebreak=index");

        if self.multi_select {
            cmd.arg("--multi");
        }
        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }
        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header);
        }

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
        parse_fzf_output(output, &item_map, self.multi_select)
    }

    pub fn select_one<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Option<T>> {
        match Self::builder().select(items)? {
            FzfResult::Selected(item) => Ok(Some(item)),
            _ => Ok(None),
        }
    }

    pub fn input(prompt: &str) -> Result<String> {
        Self::builder().prompt(prompt).input().input_dialog()
    }

    pub fn message(message: &str) -> Result<()> {
        Self::builder().message(message).message_dialog()
    }

    pub fn confirm(message: &str) -> Result<ConfirmResult> {
        Self::builder().confirm(message).confirm_dialog()
    }

    pub fn password(prompt: &str) -> Result<FzfResult<String>> {
        Self::builder().prompt(prompt).password().password_dialog()
    }

    /// Convenience method for checklist dialogs.
    /// Returns Some(checked_items) or None if cancelled.
    pub fn checklist<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Option<Vec<T>>> {
        match Self::builder()
            .checklist("Continue")
            .checklist_dialog(items)?
        {
            ChecklistResult::Confirmed(items) => Ok(Some(items)),
            ChecklistResult::Cancelled => Ok(None),
            ChecklistResult::Action(_) => Ok(None),
        }
    }
}
