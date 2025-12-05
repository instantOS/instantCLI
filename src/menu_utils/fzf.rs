//! FZF wrapper for modern fzf versions
//!
//! This module provides a wrapper around fzf targeting version 0.66.x or newer.
//!
//! ## Version Requirements
//!
//! If fzf fails with "unknown option" or similar errors indicating an old version,
//! the program will exit with a message directing the user to upgrade fzf.
//! We recommend using `mise` for managing fzf versions.
//!
//! ## Environment Handling
//!
//! All fzf invocations clear `FZF_DEFAULT_OPTS` to avoid conflicts with user/system-wide
//! settings that may contain unsupported options.

use anyhow::{self, Result};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::common::shell::shell_quote;

/// Check if the error indicates an old fzf version and exit if so
fn check_for_old_fzf_and_exit(stderr: &[u8]) {
    let stderr_str = String::from_utf8_lossy(stderr);
    if stderr_str.contains("unknown option")
        || stderr_str.contains("invalid option")
        || stderr_str.contains("invalid color specification")
        || stderr_str.contains("unrecognized option")
    {
        eprintln!("\n{}\n", "=".repeat(70));
        eprintln!("ERROR: Your fzf version is too old");
        eprintln!("{}\n", "=".repeat(70));
        eprintln!("This program requires fzf 0.66.x or newer.");
        eprintln!("Your current fzf version does not support required options.\n");
        eprintln!("To upgrade fzf, we recommend using mise:");
        eprintln!("  https://mise.jdx.dev/\n");
        eprintln!("Install mise and then run:");
        eprintln!("  mise use -g fzf@latest\n");
        eprintln!("Error details: {}", stderr_str.trim());
        eprintln!("{}\n", "=".repeat(70));
        std::process::exit(1);
    }
}

fn log_fzf_failure(stderr: &[u8], exit_code: Option<i32>) {
    if crate::ui::is_debug_enabled() {
        let stderr_str = String::from_utf8_lossy(stderr);
        let code_str = exit_code
            .map(|c| format!("exit code {}", c))
            .unwrap_or_else(|| "unknown".to_string());

        crate::ui::emit(
            crate::ui::Level::Debug,
            "fzf.execution_failed",
            &format!("FZF execution failed ({}): {}", code_str, stderr_str.trim()),
            None,
        );
    }
}

/// Extract the icon's colored background from display text and create matching padding.
/// The icon format is: \x1b[48;2;R;G;Bm\x1b[38;2;r;g;bm  {icon}  \x1b[49;39m ...
/// Returns (top_padding, bottom_padding_with_shadow) where the bottom padding has a
/// subtle darkened shadow effect using a Unicode lower block character.
fn extract_icon_padding(display: &str) -> (String, String) {
    // Look for ANSI 24-bit background color code: \x1b[48;2;R;G;Bm
    if let Some(start) = display.find("\x1b[48;2;") {
        // Find the end of the color code (the 'm')
        if let Some(end_offset) = display[start..].find('m') {
            let bg_code = &display[start..start + end_offset + 1];
            // Parse RGB values to create a darkened version for the shadow
            let rgb_part = &display[start + 7..start + end_offset]; // "R;G;B"
            let parts: Vec<&str> = rgb_part.split(';').collect();

            let reset = "\x1b[49;39m";
            let top_padding = format!("  {bg_code}       {reset}");

            // Create shadow effect using lower block character with darkened foreground
            if parts.len() == 3
                && let (Ok(r), Ok(g), Ok(b)) = (
                    parts[0].parse::<u8>(),
                    parts[1].parse::<u8>(),
                    parts[2].parse::<u8>(),
                )
            {
                let dark_r = r / 2;
                let dark_g = g / 2;
                let dark_b = b / 2;
                // Use lower one-quarter block (▂) with darkened foreground on same background
                let shadow_fg = format!("\x1b[38;2;{};{};{}m", dark_r, dark_g, dark_b);
                // The shadow character is at the bottom, creating a subtle border effect
                let bottom_with_shadow = format!("  {bg_code}{shadow_fg}▂▂▂▂▂▂▂{reset}");
                return (top_padding, bottom_with_shadow);
            }

            // Fallback if RGB parsing fails
            return (top_padding.clone(), top_padding);
        }
    }
    // Fallback: just return spaces for padding
    (" ".to_string(), " ".to_string())
}

/// Strip ANSI escape codes from a string
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we find 'm' (end of color code)
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == 'm' {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FzfPreview {
    Text(String),
    Command(String),
    None,
}

pub trait FzfSelectable {
    fn fzf_display_text(&self) -> String;

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::None
    }

    fn fzf_key(&self) -> String {
        self.fzf_display_text()
    }
}

impl FzfSelectable for String {
    fn fzf_display_text(&self) -> String {
        self.clone()
    }
}

impl FzfSelectable for &str {
    fn fzf_display_text(&self) -> String {
        self.to_string()
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
    pub fn builder() -> FzfBuilder {
        FzfBuilder::new()
    }

    fn new(
        multi_select: bool,
        prompt: Option<String>,
        header: Option<String>,
        additional_args: Vec<String>,
        initial_cursor: Option<InitialCursor>,
    ) -> Self {
        Self {
            multi_select,
            prompt,
            header,
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

        let mut item_map: HashMap<String, T> = HashMap::new();
        let mut display_lines = Vec::new();

        for item in &items {
            let display = item.fzf_display_text();
            display_lines.push(display.clone());
            item_map.insert(display.clone(), item.clone());
        }

        let cursor_position = match self.initial_cursor.as_ref() {
            Some(InitialCursor::Index(index)) => {
                if display_lines.is_empty() {
                    None
                } else {
                    let idx = *index;
                    let last = display_lines.len() - 1;
                    Some(idx.min(last))
                }
            }
            None => None,
        };

        let preview_strategy = PreviewUtils::analyze_preview_strategy(&items)?;

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

        // Configure preview based on strategy
        let input_text = match preview_strategy {
            PreviewStrategy::None => {
                // No previews - simple display
                display_lines.join("\n")
            }
            PreviewStrategy::Command(command) => {
                // Single command for all items - optimal!
                // Format: display\tkey, FZF executes command with key as $1
                // Note: bash -c 'script' name arg1 arg2
                //       where 'name' becomes $0, 'arg1' becomes $1, etc.
                cmd.arg("--delimiter=\t")
                    .arg("--with-nth=1")
                    .arg("--preview")
                    .arg(format!("{} bash \"$(echo {{}} | cut -f2)\"", command));

                display_lines
                    .iter()
                    .map(|display| {
                        // Get the key for this item
                        if let Some(item) = item_map.get(display) {
                            let key = item.fzf_key();
                            format!("{display}\t{key}")
                        } else {
                            display.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            PreviewStrategy::Text(preview_map) | PreviewStrategy::Mixed(preview_map) => {
                // Text previews or mixed - use base64 encoding
                cmd.arg("--delimiter=\t")
                    .arg("--with-nth=1")
                    .arg("--preview")
                    .arg("echo {} | cut -f2 | base64 -d");

                display_lines
                    .iter()
                    .map(|display| {
                        let preview = preview_map.get(display).cloned().unwrap_or_default();
                        let encoded_preview = general_purpose::STANDARD.encode(preview.as_bytes());
                        format!("{display}\t{encoded_preview}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };

        if let Some(position) = cursor_position {
            cmd.arg("--bind").arg(format!("load:pos({})", position + 1));
        }

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

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
                    let mut selected_items = Vec::new();
                    for line in selected_lines {
                        let display_text = line.split('\t').next().unwrap_or(line);
                        if let Some(item) = item_map.get(display_text).cloned() {
                            selected_items.push(item);
                        }
                    }
                    Ok(FzfResult::MultiSelected(selected_items))
                } else {
                    let display_text = selected_lines[0]
                        .split('\t')
                        .next()
                        .unwrap_or(selected_lines[0]);
                    if let Some(item) = item_map.get(display_text).cloned() {
                        Ok(FzfResult::Selected(item))
                    } else {
                        Ok(FzfResult::Cancelled)
                    }
                }
            }
            Err(e) => Ok(FzfResult::Error(format!("fzf execution failed: {e}"))),
        }
    }

    pub fn select_one<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Option<T>> {
        match Self::builder().select(items)? {
            FzfResult::Selected(item) => Ok(Some(item)),
            _ => Ok(None),
        }
    }

    pub fn select_many<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Vec<T>> {
        match Self::builder().multi_select(true).select(items)? {
            FzfResult::MultiSelected(items) => Ok(items),
            FzfResult::Selected(item) => Ok(vec![item]),
            _ => Ok(vec![]),
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

    pub fn password_dialog(prompt: &str) -> Result<FzfResult<String>> {
        Self::builder().prompt(prompt).password().password_dialog()
    }

    pub fn message_dialog(message: &str) -> Result<()> {
        Self::builder().message(message).message_dialog()
    }

    pub fn confirm_dialog(message: &str) -> Result<ConfirmResult> {
        Self::builder().confirm(message).confirm_dialog()
    }
}

#[derive(Debug, PartialEq)]
pub enum FzfResult<T> {
    Selected(T),
    MultiSelected(Vec<T>),
    Cancelled,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmResult {
    Yes,
    No,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct FzfBuilder {
    multi_select: bool,
    prompt: Option<String>,
    header: Option<String>,
    additional_args: Vec<String>,
    dialog_type: DialogType,
    initial_cursor: Option<InitialCursor>,
}

#[derive(Debug, Clone)]
enum DialogType {
    Selection,
    Input,
    Password {
        confirm: bool,
    },
    Confirmation {
        yes_text: String,
        no_text: String,
    },
    Message {
        ok_text: String,
        title: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum InitialCursor {
    Index(usize),
}

impl FzfBuilder {
    pub fn new() -> Self {
        Self {
            multi_select: false,
            prompt: None,
            header: None,
            additional_args: Self::default_args(),
            dialog_type: DialogType::Selection,
            initial_cursor: None,
        }
    }

    pub fn multi_select(mut self, multi: bool) -> Self {
        self.multi_select = multi;
        self
    }

    pub fn prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    pub fn header<S: Into<String>>(mut self, header: S) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn initial_index(mut self, index: usize) -> Self {
        self.initial_cursor = Some(InitialCursor::Index(index));
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.additional_args
            .extend(args.into_iter().map(Into::into));
        self
    }

    pub fn input(mut self) -> Self {
        self.dialog_type = DialogType::Input;
        self.additional_args = Self::input_args();
        self
    }

    pub fn password(mut self) -> Self {
        self.dialog_type = DialogType::Password { confirm: false };
        self.additional_args = Self::password_args();
        self
    }

    pub fn with_confirmation(mut self) -> Self {
        if let DialogType::Password { ref mut confirm } = self.dialog_type {
            *confirm = true;
        }
        self
    }

    pub fn confirm<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Confirmation {
            yes_text: "Yes".to_string(),
            no_text: "No".to_string(),
        };
        self.header = Some(message.into());
        self.additional_args = Self::confirm_args();
        self
    }

    pub fn yes_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation { yes_text, .. } = &mut self.dialog_type {
            *yes_text = text.into();
        }
        self
    }

    pub fn no_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation { no_text, .. } = &mut self.dialog_type {
            *no_text = text.into();
        }
        self
    }

    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Message {
            ok_text: "OK".to_string(),
            title: None,
        };
        self.header = Some(message.into());
        self.additional_args = Self::confirm_args();
        self
    }

    pub fn ok_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Message { ok_text, .. } = &mut self.dialog_type {
            *ok_text = text.into();
        }
        self
    }

    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        if let DialogType::Message { title: target, .. } = &mut self.dialog_type {
            *target = Some(title.into());
        }
        self
    }

    pub fn select<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        let wrapper = FzfWrapper::new(
            self.multi_select,
            self.prompt,
            self.header,
            self.additional_args,
            self.initial_cursor,
        );
        wrapper.select(items)
    }

    /// Select with vertical padding around each item.
    /// Uses NUL-separated multi-line items so the entire padded area is highlighted.
    /// This is ideal for modern, spacious menu layouts.
    /// Select with vertical padding around each item.
    /// Uses NUL-separated multi-line items so the entire padded area is highlighted.
    /// Uses FZF's {n} index for reliable item matching instead of parsing display text.
    pub fn select_padded<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
        }

        // Build NUL-separated input with padding - each item is 3 lines:
        // Line 1: blank padding with colored block
        // Line 2: content with indent
        // Line 3: blank padding with colored block + shadow effect at bottom
        let mut input_lines = Vec::new();

        for item in &items {
            let display = item.fzf_display_text();
            // Extract background color from display text to create matching padding
            // The icon badge format is: {bg}{fg}  {icon}  {reset} ...
            // We want padding lines to have the same colored block at the start,
            // with a subtle shadow on the bottom padding using a lower block character
            let (top_padding, bottom_with_shadow) = extract_icon_padding(&display);
            // Create padded multi-line item with shadow on bottom
            let padded_item = format!("{top_padding}\n  {display}\n{bottom_with_shadow}");
            input_lines.push(padded_item);
        }

        // Write previews to individual files for reliable lookup by index
        let preview_dir = std::env::temp_dir().join(format!("fzf_preview_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&preview_dir);
        {
            use std::io::Write;
            for (idx, item) in items.iter().enumerate() {
                if let FzfPreview::Text(preview) = item.fzf_preview() {
                    let preview_path = preview_dir.join(format!("{}.txt", idx));
                    if let Ok(mut file) = std::fs::File::create(&preview_path) {
                        let _ = file.write_all(preview.as_bytes());
                    }
                }
            }
        }

        // Build NUL-separated input
        let input_text = input_lines.join("\0");

        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");

        // Core options for multi-line items
        cmd.arg("--read0"); // NUL-separated input
        cmd.arg("--ansi"); // ANSI color support
        cmd.arg("--highlight-line"); // Highlight entire multi-line item
        cmd.arg("--layout=reverse");
        cmd.arg("--tiebreak=index");
        cmd.arg("--info=inline-right");

        // Use --bind to print the index on accept instead of the selection text
        // {n} is the 0-based index of the selected item
        cmd.arg("--bind").arg("enter:become(echo {n})");

        // Preview command using {n} for the 0-based item index
        let preview_cmd = format!(
            "cat {}/{{n}}.txt 2>/dev/null || echo ''",
            preview_dir.display()
        );
        cmd.arg("--preview").arg(&preview_cmd);

        // Apply prompt and header
        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }
        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header);
        }

        // Apply initial cursor position
        if let Some(InitialCursor::Index(index)) = self.initial_cursor {
            cmd.arg("--bind").arg(format!("load:pos({})", index + 1));
        }

        // Apply all additional args (styling, colors, etc.)
        for arg in &self.additional_args {
            cmd.arg(arg);
        }

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

        // Clean up temp preview directory
        let _ = std::fs::remove_dir_all(&preview_dir);

        match output {
            Ok(result) => {
                // Handle cancellation (Esc or Ctrl-C)
                if let Some(code) = result.status.code()
                    && (code == 130 || code == 143)
                {
                    return Ok(FzfResult::Cancelled);
                }

                if !result.status.success() {
                    check_for_old_fzf_and_exit(&result.stderr);
                    log_fzf_failure(&result.stderr, result.status.code());
                    return Ok(FzfResult::Cancelled);
                }

                let stdout = String::from_utf8_lossy(&result.stdout);
                let index_str = stdout.trim();

                if index_str.is_empty() {
                    return Ok(FzfResult::Cancelled);
                }

                // Parse the index from FZF's output
                if let Ok(index) = index_str.parse::<usize>()
                    && let Some(item) = items.get(index)
                {
                    return Ok(FzfResult::Selected(item.clone()));
                }

                Ok(FzfResult::Cancelled)
            }
            Err(e) => Ok(FzfResult::Error(format!("fzf execution failed: {e}"))),
        }
    }

    pub fn select_streaming(self, command: &str) -> Result<FzfResult<String>> {
        let wrapper = FzfWrapper::new(
            self.multi_select,
            self.prompt,
            self.header,
            self.additional_args,
            self.initial_cursor,
        );
        wrapper.select_streaming(command)
    }

    pub fn input_dialog(self) -> Result<String> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input()
    }

    pub fn password_dialog(self) -> Result<FzfResult<String>> {
        let confirm = if let DialogType::Password { confirm } = self.dialog_type {
            confirm
        } else {
            return Err(anyhow::anyhow!("Builder not configured for password"));
        };
        self.execute_password(confirm)
    }

    pub fn confirm_dialog(self) -> Result<ConfirmResult> {
        if !matches!(self.dialog_type, DialogType::Confirmation { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for confirmation"));
        }
        self.execute_confirm()
    }

    pub fn message_dialog(self) -> Result<()> {
        if !matches!(self.dialog_type, DialogType::Message { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for message"));
        }
        self.execute_message()
    }

    pub fn show_selection<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        self.select(items)
    }

    pub fn show_password(self) -> Result<FzfResult<String>> {
        self.password_dialog()
    }

    pub fn show_confirmation(self) -> Result<ConfirmResult> {
        self.confirm_dialog()
    }

    pub fn show_message(self) -> Result<()> {
        self.message_dialog()
    }

    pub fn input_result(self) -> Result<FzfResult<String>> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input_result()
    }

    fn execute_input(self) -> Result<String> {
        match self.execute_input_result()? {
            FzfResult::Selected(s) => Ok(s),
            FzfResult::Cancelled => Ok(String::new()),
            FzfResult::Error(e) => Err(anyhow::anyhow!(e)),
            _ => Ok(String::new()),
        }
    }

    fn execute_input_result(self) -> Result<FzfResult<String>> {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--print-query").arg("--no-info");

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} "));
        }

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(b"")?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        if let Some(code) = output.status.code()
            && (code == 130 || code == 143)
        {
            return Ok(FzfResult::Cancelled);
        }

        if !output.status.success() {
            check_for_old_fzf_and_exit(&output.stderr);
            log_fzf_failure(&output.stderr, output.status.code());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.trim_end().split('\n').collect();

        if let Some(query) = lines.first() {
            Ok(FzfResult::Selected(query.trim().to_string()))
        } else {
            Ok(FzfResult::Selected(String::new()))
        }
    }

    fn execute_password(self, confirm: bool) -> Result<FzfResult<String>> {
        loop {
            let pass1 = self.run_password_prompt(self.prompt.as_deref(), self.header.as_deref())?;

            if !confirm {
                return Ok(pass1);
            }

            let pass1_str = match pass1 {
                FzfResult::Selected(s) => s,
                _ => return Ok(pass1),
            };

            let pass2 = self.run_password_prompt(Some("Confirm password"), None)?;

            let pass2_str = match pass2 {
                FzfResult::Selected(s) => s,
                _ => return Ok(pass2),
            };

            if pass1_str == pass2_str {
                return Ok(FzfResult::Selected(pass1_str));
            }

            FzfWrapper::message("Passwords do not match. Please try again.")?;
        }
    }

    fn run_password_prompt(
        &self,
        prompt: Option<&str>,
        header: Option<&str>,
    ) -> Result<FzfResult<String>> {
        let mut cmd = Command::new("gum");
        cmd.arg("input").arg("--password");

        if let Some(p) = prompt {
            cmd.arg("--prompt").arg(format!("{p} "));
        }

        cmd.arg("--padding").arg("1 2");
        cmd.arg("--width").arg("60");

        if let Some(h) = header {
            cmd.arg("--placeholder").arg(h);
        } else {
            cmd.arg("--placeholder").arg("Enter your password");
        }

        let child = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();

        match child {
            Ok(child) => {
                let output = child.wait_with_output()?;

                if let Some(code) = output.status.code()
                    && (code == 130 || code == 143)
                {
                    return Ok(FzfResult::Cancelled);
                }

                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Ok(FzfResult::Selected(stdout.trim().to_string()))
                } else {
                    self.fallback_password_input(prompt)
                }
            }
            Err(_) => self.fallback_password_input(prompt),
        }
    }

    fn fallback_password_input(&self, prompt: Option<&str>) -> Result<FzfResult<String>> {
        use std::io::Write as _;

        eprint!("{}: ", prompt.unwrap_or("Enter password"));
        let _ = std::io::stderr().flush();

        let mut password = String::new();
        let bytes = std::io::stdin().read_line(&mut password)?;

        if bytes == 0 {
            return Ok(FzfResult::Cancelled);
        }

        Ok(FzfResult::Selected(password.trim().to_string()))
    }

    fn execute_confirm(self) -> Result<ConfirmResult> {
        let (yes_text, no_text) = if let DialogType::Confirmation {
            ref yes_text,
            ref no_text,
        } = self.dialog_type
        {
            (yes_text.clone(), no_text.clone())
        } else {
            return Ok(ConfirmResult::Cancelled);
        };

        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--layout").arg("reverse");

        if let Some(header) = &self.header {
            cmd.arg("--header").arg(format!("{header}\n"));
        }

        cmd.arg("--prompt").arg("> ");

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?;
        writeln!(stdin, "{yes_text}")?;
        writeln!(stdin, "{no_text}")?;
        stdin.flush()?;

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        if !output.status.success() {
            check_for_old_fzf_and_exit(&output.stderr);
            log_fzf_failure(&output.stderr, output.status.code());
            return Ok(ConfirmResult::Cancelled);
        }

        let selected_line = std::str::from_utf8(&output.stdout)?.trim();
        if selected_line.is_empty() {
            return Ok(ConfirmResult::Cancelled);
        }

        if selected_line == yes_text {
            Ok(ConfirmResult::Yes)
        } else if selected_line == no_text {
            Ok(ConfirmResult::No)
        } else {
            Ok(ConfirmResult::Cancelled)
        }
    }

    fn execute_message(self) -> Result<()> {
        let (ok_text, title) = if let DialogType::Message {
            ref ok_text,
            ref title,
        } = self.dialog_type
        {
            (ok_text.clone(), title.clone())
        } else {
            return Ok(());
        };

        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--layout").arg("reverse");

        if let Some(title) = &title {
            if let Some(header) = &self.header {
                cmd.arg("--header").arg(format!("{title}\n\n{header}"));
            }
        } else if let Some(header) = &self.header {
            cmd.arg("--header").arg(header);
        }

        cmd.arg("--prompt").arg("- ");

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(ok_text.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        if let Some(code) = output.status.code()
            && (code == 130 || code == 143)
        {
            return Ok(());
        }

        if !output.status.success() {
            check_for_old_fzf_and_exit(&output.stderr);
        }

        Ok(())
    }

    fn default_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "10%,2%".to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(Self::theme_args());
        args
    }

    fn input_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "20%,2%".to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(Self::theme_args());
        args
    }

    fn confirm_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "20%,2%".to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(Self::theme_args());
        args.push("--info=hidden".to_string());
        args.push("--color=header:-1".to_string());
        args
    }

    fn password_args() -> Vec<String> {
        vec![]
    }

    fn theme_args() -> Vec<String> {
        vec![
            "--color=bg+:#313244".to_string(),
            "--color=bg:#1E1E2E".to_string(),
            "--color=spinner:#F5E0DC".to_string(),
            "--color=hl:#F38BA8".to_string(),
            "--color=fg:#CDD6F4".to_string(),
            "--color=header:#F38BA8".to_string(),
            "--color=info:#CBA6F7".to_string(),
            "--color=pointer:#F5E0DC".to_string(),
            "--color=marker:#B4BEFE".to_string(),
            "--color=fg+:#CDD6F4".to_string(),
            "--color=prompt:#CBA6F7".to_string(),
            "--color=hl+:#F38BA8".to_string(),
            "--color=selected-bg:#45475A".to_string(),
            "--color=border:#6C7086".to_string(),
            "--color=label:#CDD6F4".to_string(),
        ]
    }
}

#[derive(Serialize, Deserialize)]
struct PreviewData {
    key: String,
    preview_type: String,
    preview_content: String,
}

pub struct PreviewUtils;

pub(crate) enum PreviewStrategy {
    /// No previews
    None,
    /// Text previews embedded in input (base64)
    Text(HashMap<String, String>),
    /// Single command executed by FZF with key substitution
    Command(String),
    /// Mixed - each item has different preview (fallback to text encoding)
    Mixed(HashMap<String, String>),
}

impl PreviewUtils {
    /// Analyze preview types and determine optimal strategy
    pub fn analyze_preview_strategy<T: FzfSelectable>(items: &[T]) -> Result<PreviewStrategy> {
        if items.is_empty() {
            return Ok(PreviewStrategy::None);
        }

        let mut first_command: Option<String> = None;
        let mut text_map = HashMap::new();
        let mut has_text = false;
        let mut has_command = false;
        let mut all_same_command = true;

        for item in items {
            let display = item.fzf_display_text();
            let key = item.fzf_key();

            match item.fzf_preview() {
                FzfPreview::Text(text) => {
                    has_text = true;
                    text_map.insert(display.clone(), text);
                }
                FzfPreview::Command(cmd) => {
                    has_command = true;
                    if let Some(ref first) = first_command {
                        if first != &cmd {
                            all_same_command = false;
                        }
                    } else {
                        first_command = Some(cmd.clone());
                    }
                    // For command previews, we'll pass the key (usually MIME type) to the command
                    // The command should use $1 to reference it
                    text_map.insert(display.clone(), key);
                }
                FzfPreview::None => {
                    // No preview for this item
                }
            }
        }

        // Determine strategy
        if !has_text && !has_command {
            Ok(PreviewStrategy::None)
        } else if has_command && !has_text && all_same_command {
            // All items use the same command - optimal case!
            // We can use a single --preview command with {} substitution
            Ok(PreviewStrategy::Command(first_command.unwrap()))
        } else if !has_command && has_text {
            // All text previews - use existing base64 approach
            Ok(PreviewStrategy::Text(text_map))
        } else {
            // Mixed or multiple different commands - fall back to text encoding
            // For commands, we store the key to pass to a wrapper script
            Ok(PreviewStrategy::Mixed(text_map))
        }
    }
}
