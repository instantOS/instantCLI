//! FZF wrapper and selection logic

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::common::shell::shell_quote;

use super::types::*;
use super::preview::PreviewUtils;
use super::preview::PreviewStrategy;
use super::utils::{check_for_old_fzf_and_exit, log_fzf_failure};

// ============================================================================
// Helper functions for FzfWrapper::select
// ============================================================================

/// Build a lookup map from display text to item, and collect display lines.
fn build_item_map<T: FzfSelectable + Clone>(items: &[T]) -> (HashMap<String, T>, Vec<String>) {
    let mut item_map: HashMap<String, T> = HashMap::new();
    let mut display_lines = Vec::new();

    for item in items {
        let display = item.fzf_display_text();
        display_lines.push(display.clone());
        item_map.insert(display.clone(), item.clone());
    }

    (item_map, display_lines)
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
fn configure_preview_and_input<T: FzfSelectable>(
    cmd: &mut Command,
    strategy: PreviewStrategy,
    display_lines: &[String],
    item_map: &HashMap<String, T>,
) -> String {
    match strategy {
        PreviewStrategy::None => display_lines.join("\n"),
        PreviewStrategy::Command(command) => {
            cmd.arg("--delimiter=\t")
                .arg("--with-nth=1")
                .arg("--preview")
                .arg(format!("{} bash \"$(echo {{}} | cut -f2)\"", command));

            display_lines
                .iter()
                .map(|display| {
                    if let Some(item) = item_map.get(display) {
                        format!("{display}\t{}", item.fzf_key())
                    } else {
                        display.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        PreviewStrategy::Text(preview_map) | PreviewStrategy::Mixed(preview_map) => {
            cmd.arg("--delimiter=\t")
                .arg("--with-nth=1")
                .arg("--preview")
                .arg("echo {} | cut -f2 | base64 -d");

            display_lines
                .iter()
                .map(|display| {
                    let preview = preview_map.get(display).cloned().unwrap_or_default();
                    let encoded = general_purpose::STANDARD.encode(preview.as_bytes());
                    format!("{display}\t{encoded}")
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

/// Parse fzf output and map selected lines back to items.
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

    if multi_select {
        let selected_items: Vec<T> = selected_lines
            .iter()
            .filter_map(|line| {
                let display_text = line.split('\t').next().unwrap_or(line);
                item_map.get(display_text).cloned()
            })
            .collect();
        Ok(FzfResult::MultiSelected(selected_items))
    } else {
        let display_text = selected_lines[0]
            .split('\t')
            .next()
            .unwrap_or(selected_lines[0]);
        match item_map.get(display_text).cloned() {
            Some(item) => Ok(FzfResult::Selected(item)),
            None => Ok(FzfResult::Cancelled),
        }
    }
}

pub struct FzfWrapper {
    pub(crate) multi_select: bool,
    pub(crate) prompt: Option<String>,
    pub(crate) header: Option<String>,
    pub(crate) additional_args: Vec<String>,
    pub(crate) initial_cursor: Option<InitialCursor>,
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
    ) -> Self {
        Self {
            multi_select,
            prompt,
            header: header.map(|h| h.to_fzf_string()),
            additional_args,
            initial_cursor,
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

        // Build item lookup map and display lines
        let (item_map, display_lines) = build_item_map(&items);

        // Calculate initial cursor position
        let cursor_position = calculate_cursor_position(&self.initial_cursor, display_lines.len());

        // Analyze preview strategy and build input text
        let preview_strategy = PreviewUtils::analyze_preview_strategy(&items)?;

        // Configure fzf command
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
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
            configure_preview_and_input(&mut cmd, preview_strategy, &display_lines, &item_map);

        if let Some(position) = cursor_position {
            cmd.arg("--bind").arg(format!("load:pos({})", position + 1));
        }
        for arg in &self.additional_args {
            cmd.arg(arg);
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
