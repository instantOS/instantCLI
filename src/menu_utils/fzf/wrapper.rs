//! FZF wrapper and selection logic

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::common::shell::shell_quote;

use super::preview::PreviewStrategy;
use super::preview::PreviewUtils;
use super::types::*;
use super::utils::{check_for_old_fzf_and_exit, log_fzf_failure};

// ============================================================================
// Helper functions for FzfWrapper::select
// ============================================================================

/// Build a lookup map from fzf_key to item, and collect display lines with keys.
fn build_item_map<T: FzfSelectable + Clone>(items: &[T]) -> (HashMap<String, T>, Vec<(String, String)>) {
    let mut item_map: HashMap<String, T> = HashMap::new();
    let mut display_with_keys = Vec::new();

    for item in items {
        let display = item.fzf_display_text();
        let key = item.fzf_key();
        display_with_keys.push((display, key.clone()));
        item_map.insert(key, item.clone());
    }

    (item_map, display_with_keys)
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

/// Configure fzf preview and build input text based on the preview strategy.
/// Always includes the fzf_key after a tab so we can reliably match items.
fn configure_preview_and_input(
    cmd: &mut Command,
    strategy: PreviewStrategy,
    display_with_keys: &[(String, String)],
) -> String {
    // Always use Unit Separator (\x1f) delimiter to separate display from key
    cmd.arg("--delimiter=\x1f").arg("--with-nth=1");

    match strategy {
        PreviewStrategy::None => {
            // Format: display\x1fkey
            display_with_keys
                .iter()
                .map(|(display, key)| format!("{display}\x1f{key}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::Command(command) => {
            // Use literal \x1f character for POSIX compatibility (works in dash/sh)
            cmd.arg("--preview")
                .arg(format!("{} bash \"$(echo {{}} | cut -d'\x1f' -f2)\"", command));

            // Format: display\x1fkey
            display_with_keys
                .iter()
                .map(|(display, key)| format!("{display}\x1f{key}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::Text(preview_map) | PreviewStrategy::Mixed(preview_map) => {
            // Use field 3 for base64-encoded preview
            // Use literal \x1f character for POSIX compatibility
            cmd.arg("--preview")
                .arg("echo {} | cut -d'\x1f' -f3 | base64 -d");

            // Format: display\x1fkey\x1fbase64_preview
            display_with_keys
                .iter()
                .map(|(display, key)| {
                    let preview = preview_map.get(display).cloned().unwrap_or_default();
                    let encoded = general_purpose::STANDARD.encode(preview.as_bytes());
                    format!("{display}\x1f{key}\x1f{encoded}")
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
        log_fzf_failure(&result.stderr, result.status.code());
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

    // Extract the key from field 2 (format: display\x1fkey or display\x1fkey\x1fpreview)
    if multi_select {
        let selected_items: Vec<T> = selected_lines
            .iter()
            .filter_map(|line| {
                // Extract key from field 2 (index 1) using Unit Separator
                let key = line.split('\x1f').nth(1)?;
                item_map.get(key).cloned()
            })
            .collect();
        Ok(FzfResult::MultiSelected(selected_items))
    } else {
        // Extract key from field 2 (index 1) using Unit Separator
        if let Some(key) = selected_lines[0].split('\x1f').nth(1) {
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
                    log_fzf_failure(&result.stderr, result.status.code());
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

        // Build item lookup map (keyed by fzf_key) and display lines with keys
        let (item_map, display_with_keys) = build_item_map(&items);

        // Calculate initial cursor position
        let cursor_position = calculate_cursor_position(&self.initial_cursor, display_with_keys.len());

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
            configure_preview_and_input(&mut cmd, preview_strategy, &display_with_keys);

        if let Some(position) = cursor_position {
            cmd.arg("--bind").arg(format!("load:pos({})", position + 1));
        }
        for arg in &self.additional_args {
            cmd.arg(arg);
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
}
