use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::process::Command;

/// Preview type for fzf items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FzfPreview {
    Text(String),
    Command(String), // shell command for preview like for example git branch info
    None,            // empty preview for this item
}

/// Core trait that types must implement to be selectable with fzf
pub trait FzfSelectable {
    /// The text that appears in the fzf selection list
    fn fzf_display_text(&self) -> String;

    /// Preview content shown in the preview window
    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::None
    }

    /// Optional key for identifying this item (used internally for mapping)
    /// Defaults to using the display text as the key
    fn fzf_key(&self) -> String {
        self.fzf_display_text()
    }
}

/// Simplified FZF wrapper with unified builder pattern
pub struct FzfWrapper {
    multi_select: bool,
    prompt: Option<String>,
    header: Option<String>,
    additional_args: Vec<String>,
}

impl FzfWrapper {
    /// Create a new builder for configuring FZF
    pub fn builder() -> FzfBuilder {
        FzfBuilder::new()
    }
}

/// Result of fzf selection
#[derive(Debug)]
pub enum FzfResult<T> {
    Selected(T),
    MultiSelected(Vec<T>),
    Cancelled,
    Error(String),
}

/// Result of confirmation dialog
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmResult {
    Yes,
    No,
    Cancelled,
}

/// Unified builder for all FZF operations
#[derive(Debug, Clone)]
pub struct FzfBuilder {
    multi_select: bool,
    prompt: Option<String>,
    header: Option<String>,
    additional_args: Vec<String>,
    dialog_type: DialogType,
}

#[derive(Debug, Clone)]
enum DialogType {
    Selection,
    Input,
    Confirmation {
        yes_text: String,
        no_text: String,
    },
    Message {
        ok_text: String,
        title: Option<String>,
    },
}

impl FzfBuilder {
    /// Create a new builder with default selection options
    pub fn new() -> Self {
        Self {
            multi_select: false,
            prompt: None,
            header: None,
            additional_args: Self::default_args(),
            dialog_type: DialogType::Selection,
        }
    }

    /// Enable multi-select mode
    pub fn multi_select(mut self, multi: bool) -> Self {
        self.multi_select = multi;
        self
    }

    /// Set the prompt text
    pub fn prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Set the header text (supports multi-line)
    pub fn header<S: Into<String>>(mut self, header: S) -> Self {
        self.header = Some(header.into());
        self
    }

    /// Add additional fzf arguments
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.additional_args
            .extend(args.into_iter().map(Into::into));
        self
    }

    /// Configure for text input mode
    pub fn input(mut self) -> Self {
        self.dialog_type = DialogType::Input;
        self.additional_args = Self::input_args();
        self
    }

    /// Configure for confirmation dialog
    pub fn confirm<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Confirmation {
            yes_text: "Yes".to_string(),
            no_text: "No".to_string(),
        };
        self.header = Some(message.into());
        self.additional_args = Self::confirm_args();
        self
    }

    /// Set custom yes/no text for confirmation
    pub fn yes_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation {
            yes_text,
            no_text: _,
        } = &mut self.dialog_type
        {
            *yes_text = text.into();
        }
        self
    }

    /// Set custom no text for confirmation
    pub fn no_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation {
            yes_text: _,
            no_text,
        } = &mut self.dialog_type
        {
            *no_text = text.into();
        }
        self
    }

    /// Configure for message dialog
    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Message {
            ok_text: "OK".to_string(),
            title: None,
        };
        self.header = Some(message.into());
        self.additional_args = Self::confirm_args();
        self
    }

    /// Set custom OK text for message
    pub fn ok_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Message { ok_text, title: _ } = &mut self.dialog_type {
            *ok_text = text.into();
        }
        self
    }

    /// Set title for message dialog
    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        if let DialogType::Message {
            ok_text: _,
            title: existing_title,
        } = &mut self.dialog_type
        {
            *existing_title = Some(title.into());
        }
        self
    }

    /// Execute selection dialog
    pub fn select<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        let wrapper = FzfWrapper {
            multi_select: self.multi_select,
            prompt: self.prompt,
            header: self.header,
            additional_args: self.additional_args,
        };
        wrapper.select(items)
    }

    /// Execute input dialog
    pub fn input_dialog(self) -> Result<String> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input()
    }

    /// Execute confirmation dialog
    pub fn confirm_dialog(self) -> Result<ConfirmResult> {
        if !matches!(self.dialog_type, DialogType::Confirmation { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for confirmation"));
        }
        self.execute_confirm()
    }

    /// Execute confirmation dialog
    pub fn show(self) -> Result<ConfirmResult> {
        self.confirm_dialog()
    }

    /// Execute message dialog
    pub fn message_dialog(self) -> Result<()> {
        if !matches!(self.dialog_type, DialogType::Message { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for message"));
        }
        self.execute_message()
    }

    /// Default arguments for selection
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

    /// Arguments for input dialogs
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

    /// Arguments for confirmation/message dialogs
    fn confirm_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "20%,2%".to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(Self::theme_args());
        args
    }

    /// Catppuccin theme colors
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

// Internal execution methods for FzfBuilder
impl FzfBuilder {
    fn execute_input(self) -> Result<String> {
        let mut cmd = Command::new("fzf");
        cmd.arg("--print-query").arg("--no-info");

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} "));
        }

        for arg in self.additional_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_fzf_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(b"")?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_fzf_process(pid);

        if let Some(code) = output.status.code() {
            if code == 130 || code == 143 {
                return Ok(String::new());
            }
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.trim().split('\n').collect();

        if let Some(query) = lines.first() {
            Ok(query.trim().to_string())
        } else {
            Ok(String::new())
        }
    }

    fn execute_confirm(self) -> Result<ConfirmResult> {
        let (yes_text, no_text) =
            if let DialogType::Confirmation { yes_text, no_text } = self.dialog_type {
                (yes_text, no_text)
            } else {
                return Ok(ConfirmResult::Cancelled);
            };

        let mut cmd = Command::new("fzf");
        cmd.arg("--layout").arg("reverse");

        if let Some(header) = &self.header {
            cmd.arg("--header").arg(format!("{header}\n"));
        }

        cmd.arg("--prompt").arg("> ");

        for arg in self.additional_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_fzf_process(pid);

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?;
        writeln!(stdin, "{yes_text}")?;
        writeln!(stdin, "{no_text}")?;
        stdin.flush()?;

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_fzf_process(pid);

        if !output.status.success() {
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
        let (ok_text, title) = if let DialogType::Message { ok_text, title } = self.dialog_type {
            (ok_text, title)
        } else {
            return Ok(());
        };

        let mut cmd = Command::new("fzf");
        cmd.arg("--layout").arg("reverse");

        if let Some(title) = &title {
            if let Some(header) = &self.header {
                cmd.arg("--header").arg(format!("{}\n\n{}", title, header));
            }
        } else if let Some(header) = &self.header {
            cmd.arg("--header").arg(header);
        }

        cmd.arg("--prompt").arg("- ");

        for arg in self.additional_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_fzf_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(ok_text.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_fzf_process(pid);

        if let Some(code) = output.status.code() {
            if code == 130 || code == 143 {
                return Ok(());
            }
        }

        Ok(())
    }
}

/// Internal structure for JSON serialization to the preview script
#[derive(Serialize, Deserialize)]
struct PreviewData {
    key: String,
    preview_type: String,
    preview_content: String,
}

/// Shared preview generation utilities
pub struct PreviewUtils;

impl PreviewUtils {
    /// Build a simple preview mapping using display text directly with better escaping
    pub fn build_preview_mapping<T: FzfSelectable>(items: &[T]) -> Result<HashMap<String, String>> {
        let mut preview_map = HashMap::new();

        for item in items {
            let display_text = item.fzf_display_text();
            match item.fzf_preview() {
                FzfPreview::Text(text) => {
                    preview_map.insert(display_text, text);
                }
                FzfPreview::Command(cmd) => {
                    preview_map.insert(display_text, cmd);
                }
                FzfPreview::None => {}
            }
        }

        Ok(preview_map)
    }
}

impl FzfWrapper {
    fn new(
        multi_select: bool,
        prompt: Option<String>,
        header: Option<String>,
        additional_args: Vec<String>,
    ) -> Self {
        Self {
            multi_select,
            prompt,
            header,
            additional_args,
        }
    }

    /// Select from a vector of FzfSelectable items
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

        let preview_map = PreviewUtils::build_preview_mapping(&items)?;
        let has_previews = !preview_map.is_empty();

        let mut cmd = Command::new("fzf");
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

        if has_previews {
            cmd.arg("--delimiter=\t")
                .arg("--with-nth=1")
                .arg("--preview")
                .arg("echo {} | cut -f2 | base64 -d");
        }

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        let input_text = if has_previews {
            display_lines
                .iter()
                .map(|display| {
                    let preview = preview_map.get(display).cloned().unwrap_or_default();
                    let encoded_preview = general_purpose::STANDARD.encode(preview.as_bytes());
                    format!("{display}\t{encoded_preview}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            display_lines.join("\n")
        };

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_fzf_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(input_text.as_bytes())?;
        }

        let output = child.wait_with_output();
        crate::menu::server::unregister_fzf_process(pid);

        match output {
            Ok(result) => {
                if let Some(code) = result.status.code() {
                    if code == 130 || code == 143 {
                        return Ok(FzfResult::Cancelled);
                    }
                }

                let stdout = String::from_utf8_lossy(&result.stdout);
                let selected_lines: Vec<&str> = stdout
                    .trim()
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
}

// Convenience static methods
impl FzfWrapper {
    /// Quick single selection with default options
    pub fn select_one<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Option<T>> {
        match Self::builder().select(items)? {
            FzfResult::Selected(item) => Ok(Some(item)),
            _ => Ok(None),
        }
    }

    /// Quick multi-selection with default options
    pub fn select_many<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Vec<T>> {
        match Self::builder().multi_select(true).select(items)? {
            FzfResult::MultiSelected(items) => Ok(items),
            FzfResult::Selected(item) => Ok(vec![item]),
            _ => Ok(vec![]),
        }
    }

    /// Text input mode for getting user input
    pub fn input(prompt: &str) -> Result<String> {
        Self::builder().prompt(prompt).input().input_dialog()
    }

    /// Display a popup message with OK button
    pub fn message(message: &str) -> Result<()> {
        Self::builder().message(message).message_dialog()
    }

    /// Confirmation dialog with yes/no options
    pub fn confirm(message: &str) -> Result<ConfirmResult> {
        Self::builder().confirm(message).confirm_dialog()
    }

    /// Input dialog with custom prompt
    pub fn input_dialog(prompt: &str) -> Result<String> {
        Self::builder().prompt(prompt).input().input_dialog()
    }

    /// Message dialog with custom text
    pub fn message_dialog(message: &str) -> Result<()> {
        Self::builder().message(message).message_dialog()
    }

    /// Confirmation dialog with custom message
    pub fn confirm_dialog(message: &str) -> Result<ConfirmResult> {
        Self::builder().confirm(message).confirm_dialog()
    }
}

// Example implementations
#[derive(Debug, Clone)]
pub struct FileItem {
    pub path: String,
    pub size: u64,
    pub modified: String,
}

impl FzfSelectable for FileItem {
    fn fzf_display_text(&self) -> String {
        format!("{} ({})", self.path, self.size)
    }

    fn fzf_preview(&self) -> FzfPreview {
        std::fs::read_to_string(&self.path)
            .ok()
            .map(|content| {
                if content.len() > 1000 {
                    FzfPreview::Text(format!("{}...\n\n[File truncated]", &content[..1000]))
                } else {
                    FzfPreview::Text(content)
                }
            })
            .unwrap_or_else(|| {
                FzfPreview::Text(format!("Binary file or read error: {}", self.path))
            })
    }
}
