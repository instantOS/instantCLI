use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::process::Command;
use tempfile::{NamedTempFile, TempPath};

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

/// Configuration for fzf behavior
#[derive(Debug, Clone)]
pub struct FzfOptions {
    pub multi_select: bool,
    pub prompt: Option<String>,
    pub header: Option<String>,
    pub additional_args: Vec<String>,
}

impl Default for FzfOptions {
    fn default() -> Self {
        Self {
            multi_select: false,
            prompt: None,
            header: None,
            additional_args: Self::default_margin_args(),
        }
    }
}

impl FzfOptions {
    /// Create a new FzfOptions with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set multi-select mode
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

    /// Add additional arguments
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.additional_args
            .extend(args.into_iter().map(Into::into));
        self
    }

    /// Default margin arguments used across the application
    fn default_margin_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "10%,2%".to_string(), // 10% vertical, 2% horizontal
            "--min-height".to_string(),
            "10".to_string(),
        ];

        // Add catppuccin theme colors
        args.extend(Self::catppuccin_theme_args());
        args
    }

    /// Catppuccin theme colors for fzf
    fn catppuccin_theme_args() -> Vec<String> {
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

    /// Margin arguments for input dialogs (larger vertical margin)
    fn input_margin_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "20%,2%".to_string(), // 20% vertical, 2% horizontal
            "--min-height".to_string(),
            "10".to_string(),
        ];

        // Add catppuccin theme colors
        args.extend(Self::catppuccin_theme_args());
        args
    }

    /// Margin arguments for confirmation dialogs (largest vertical margin)
    fn confirm_margin_args() -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            "40%,2%".to_string(), // 40% vertical, 2% horizontal
            "--min-height".to_string(),
            "10".to_string(),
        ];

        // Add catppuccin theme colors
        args.extend(Self::catppuccin_theme_args());
        args
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

/// Builder for creating FzfWrapper instances with ergonomic configuration
#[derive(Debug, Clone)]
pub struct FzfWrapperBuilder {
    options: FzfOptions,
}

impl FzfWrapperBuilder {
    /// Create a new builder with default options
    pub fn new() -> Self {
        Self {
            options: FzfOptions::default(),
        }
    }

    /// Enable multi-select mode
    pub fn multi_select(mut self) -> Self {
        self.options.multi_select = true;
        self
    }

    /// Set the prompt text
    pub fn prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.options.prompt = Some(prompt.into());
        self
    }

    /// Set the header text (supports multi-line)
    pub fn header<S: Into<String>>(mut self, header: S) -> Self {
        self.options.header = Some(header.into());
        self
    }

    /// Add additional fzf arguments
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.options
            .additional_args
            .extend(args.into_iter().map(Into::into));
        self
    }

    /// Use default margin styling
    pub fn default_margin(self) -> Self {
        self.args(FzfOptions::default_margin_args())
    }

    /// Use input dialog margin styling
    pub fn input_margin(self) -> Self {
        self.args(FzfOptions::input_margin_args())
    }

    /// Use confirmation dialog margin styling
    pub fn confirm_margin(self) -> Self {
        self.args(FzfOptions::confirm_margin_args())
    }

    /// Build the FzfWrapper instance
    pub fn build(self) -> FzfWrapper {
        FzfWrapper::with_options(self.options)
    }

    /// Convenience method to select items directly
    pub fn select<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        self.build().select(items)
    }

    /// Convenience method to select one item
    pub fn select_one<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<Option<T>> {
        FzfWrapper::select_one(items)
    }

    /// Convenience method to select multiple items
    pub fn select_many<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<Vec<T>> {
        FzfWrapper::select_many(items)
    }
}

/// Main fzf wrapper struct
pub struct FzfWrapper {
    options: FzfOptions,
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
    /// Create a temporary preview script that can handle preview data
    pub fn create_preview_script(
        preview_map: HashMap<String, (String, String)>,
    ) -> Result<(TempPath, std::path::PathBuf)> {
        let mut temp_file = NamedTempFile::new()?;

        // Write a shell script that handles both text and command previews
        let script_content = format!(
            r#"#!/bin/bash
key="$1"
case "$key" in
{}
*)
    echo "No preview available"
    ;;
esac
"#,
            preview_map
                .iter()
                .map(|(key, (preview_type, preview_content))| {
                    let escaped_key = key.replace("'", "'\\''");
                    let escaped_content = preview_content.replace("'", "'\\''");
                    match preview_type.as_str() {
                        "text" => format!("'{escaped_key}') echo '{escaped_content}' ;;"),
                        "command" => format!("'{escaped_key}') {escaped_content} ;;"),
                        _ => format!("'{escaped_key}') echo 'Invalid preview type' ;;"),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        );

        temp_file.write_all(script_content.as_bytes())?;
        temp_file.flush()?;

        // Get the path before we do anything else
        let script_path = temp_file.path().to_path_buf();

        // Make the script executable while it's still open
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        let temp_path = temp_file.into_temp_path();
        Ok((temp_path, script_path))
    }

    /// Build preview map from items that implement FzfSelectable
    pub fn build_preview_map<T: FzfSelectable>(items: &[T]) -> HashMap<String, (String, String)> {
        let mut preview_map = HashMap::new();

        for item in items {
            let key = item.fzf_key();
            match item.fzf_preview() {
                FzfPreview::Text(text) => {
                    preview_map.insert(key, ("text".to_string(), text));
                }
                FzfPreview::Command(cmd) => {
                    preview_map.insert(key, ("command".to_string(), cmd));
                }
                FzfPreview::None => {}
            }
        }

        preview_map
    }
}

impl FzfWrapper {
    pub fn new() -> Self {
        Self {
            options: FzfOptions::default(),
        }
    }

    pub fn with_options(options: FzfOptions) -> Self {
        Self { options }
    }

    /// Create a builder for configuring FzfWrapper
    pub fn builder() -> FzfWrapperBuilder {
        FzfWrapperBuilder::new()
    }

    /// Select from a vector of FzfSelectable items
    pub fn select<T: FzfSelectable + Clone>(&self, items: Vec<T>) -> Result<FzfResult<T>> {
        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
        }

        // Create a mapping from display text to original items
        let mut item_map: HashMap<String, T> = HashMap::new();
        let mut display_lines = Vec::new();

        for item in &items {
            let key = item.fzf_key();
            let display = item.fzf_display_text();

            display_lines.push(display.clone());
            item_map.insert(key.clone(), item.clone());
        }

        // Create preview script if any items have preview content and keep it alive
        let preview_map = PreviewUtils::build_preview_map(&items);
        let _preview_script_keeper = if !preview_map.is_empty() {
            Some(PreviewUtils::create_preview_script(preview_map)?)
        } else {
            None
        };

        let preview_script_path = _preview_script_keeper.as_ref().map(|(_, path)| path);

        // Build fzf command
        let mut cmd = Command::new("fzf");
        cmd.arg("--tiebreak=index");

        if self.options.multi_select {
            cmd.arg("--multi");
        }

        if let Some(prompt) = &self.options.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }

        if let Some(header) = &self.options.header {
            cmd.arg("--header").arg(header);
        }

        // Add preview if we have a preview script
        if let Some(script_path) = preview_script_path {
            cmd.arg("--preview")
                .arg(format!("{} {{}}", script_path.display()));
        }

        // Add additional arguments
        for arg in &self.options.additional_args {
            cmd.arg(arg);
        }

        // Execute fzf with process tracking
        let input_text = display_lines.join("\n");

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        // Register the process for potential killing if scratchpad becomes invisible
        let pid = child.id();
        let _ = crate::menu::server::register_fzf_process(pid);

        // Write input to stdin
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(input_text.as_bytes())?;
        }

        // Wait for the process to complete
        let output = child.wait_with_output();

        // Unregister the process since it's done
        crate::menu::server::unregister_fzf_process(pid);

        // The preview script is kept alive by _preview_script_keeper
        // which will be dropped when this function ends, after fzf has finished

        match output {
            Ok(result) => {
                // Check if fzf was cancelled (exit code 130) or killed (exit code 143)
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
                } else if self.options.multi_select {
                    let mut selected_items = Vec::new();
                    for line in selected_lines {
                        if let Some(item) = item_map.get(line).cloned() {
                            selected_items.push(item);
                        }
                    }
                    Ok(FzfResult::MultiSelected(selected_items))
                } else if let Some(item) = item_map.get(selected_lines[0]).cloned() {
                    Ok(FzfResult::Selected(item))
                } else {
                    Ok(FzfResult::Cancelled)
                }
            }
            Err(e) => Ok(FzfResult::Error(format!("fzf execution failed: {e}"))),
        }
    }
}

// Convenience methods
impl FzfWrapper {
    /// Quick single selection with default options
    pub fn select_one<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Option<T>> {
        let wrapper = FzfWrapper::new();
        match wrapper.select(items)? {
            FzfResult::Selected(item) => Ok(Some(item)),
            _ => Ok(None),
        }
    }

    /// Quick multi-selection with default options
    pub fn select_many<T: FzfSelectable + Clone>(items: Vec<T>) -> Result<Vec<T>> {
        let wrapper = FzfWrapper::with_options(FzfOptions {
            multi_select: true,
            ..Default::default()
        });
        match wrapper.select(items)? {
            FzfResult::MultiSelected(items) => Ok(items),
            FzfResult::Selected(item) => Ok(vec![item]),
            _ => Ok(vec![]),
        }
    }

    /// Text input mode for getting user input
    pub fn input(prompt: &str) -> Result<String> {
        let mut cmd = Command::new("fzf");
        cmd.arg("--print-query")
            .arg("--no-info")
            .arg("--prompt")
            .arg(format!("{prompt} "));

        // Add input-specific margin arguments
        for arg in FzfOptions::input_margin_args() {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        // Register the process for potential killing if scratchpad becomes invisible
        let pid = child.id();
        let _ = crate::menu::server::register_fzf_process(pid);

        // Write empty input to stdin
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(b"")?;
        }

        // Wait for the process to complete
        let output = child.wait_with_output()?;

        // Unregister the process since it's done
        crate::menu::server::unregister_fzf_process(pid);

        // Check if fzf was cancelled (exit code 130) or killed (exit code 143)
        if let Some(code) = output.status.code() {
            if code == 130 || code == 143 {
                return Ok(String::new());
            }
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.trim().split('\n').collect();

        // With --print-query, the first line is the query
        if let Some(query) = lines.first() {
            Ok(query.trim().to_string())
        } else {
            Ok(String::new())
        }
    }

    /// Display a popup message with OK button
    pub fn message(message: &str) -> Result<()> {
        Self::message_builder().message(message).show()
    }

    /// Create a new message dialog builder
    pub fn message_builder() -> MessageDialogBuilder {
        MessageDialogBuilder::new()
    }

    /// Confirmation dialog with yes/no options
    pub fn confirm(message: &str) -> Result<ConfirmResult> {
        Self::confirm_builder().message(message).show()
    }

    /// Create a new confirmation dialog builder
    pub fn confirm_builder() -> ConfirmationDialogBuilder {
        ConfirmationDialogBuilder::new()
    }
}

/// Builder for creating confirmation dialogs with multi-line support
#[derive(Debug, Clone)]
pub struct ConfirmationDialogBuilder {
    message: String,
    yes_text: String,
    no_text: String,
    default_yes: bool,
    custom_options: Vec<ConfirmationItem>,
}

impl ConfirmationDialogBuilder {
    /// Create a new confirmation dialog builder
    pub fn new() -> Self {
        Self {
            message: String::new(),
            yes_text: "Yes".to_string(),
            no_text: "No".to_string(),
            default_yes: true,
            custom_options: Vec::new(),
        }
    }

    /// Set the confirmation message (supports multi-line text)
    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.message = message.into();
        self
    }

    /// Set custom Yes button text
    pub fn yes_text<S: Into<String>>(mut self, text: S) -> Self {
        self.yes_text = text.into();
        self
    }

    /// Set custom No button text
    pub fn no_text<S: Into<String>>(mut self, text: S) -> Self {
        self.no_text = text.into();
        self
    }

    /// Set default selection (true = Yes, false = No)
    pub fn default_yes(mut self, default: bool) -> Self {
        self.default_yes = default;
        self
    }

    /// Add custom confirmation options
    pub fn custom_options(mut self, options: Vec<ConfirmationItem>) -> Self {
        self.custom_options = options;
        self
    }

    /// Show the confirmation dialog
    pub fn show(self) -> Result<ConfirmResult> {
        let yes_text = self.yes_text.clone();
        let no_text = self.no_text.clone();

        let items = if self.custom_options.is_empty() {
            vec![
                ConfirmationItem {
                    value: ConfirmResult::Yes,
                    text: yes_text.clone(),
                },
                ConfirmationItem {
                    value: ConfirmResult::No,
                    text: no_text.clone(),
                },
            ]
        } else {
            self.custom_options
        };

        let wrapper = FzfWrapper::with_options(FzfOptions {
            header: Some(self.message),
            prompt: Some(if self.default_yes {
                format!("> ({yes_text}) ")
            } else {
                format!("> ({no_text}) ")
            }),
            additional_args: FzfOptions::confirm_margin_args(),
            ..Default::default()
        });

        match wrapper.select(items)? {
            FzfResult::Selected(item) => Ok(item.value),
            FzfResult::Cancelled => {
                // When user cancels (ESC), we return Cancelled instead of using default
                Ok(ConfirmResult::Cancelled)
            }
            _ => {
                // For any other error cases, return Cancelled
                Ok(ConfirmResult::Cancelled)
            }
        }
    }
}

/// Builder for creating message dialogs with multi-line support
#[derive(Debug, Clone)]
pub struct MessageDialogBuilder {
    message: String,
    ok_text: String,
    title: Option<String>,
}

impl MessageDialogBuilder {
    /// Create a new message dialog builder
    pub fn new() -> Self {
        Self {
            message: String::new(),
            ok_text: "OK".to_string(),
            title: None,
        }
    }

    /// Set the message content (supports multi-line text)
    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.message = message.into();
        self
    }

    /// Set custom OK button text
    pub fn ok_text<S: Into<String>>(mut self, text: S) -> Self {
        self.ok_text = text.into();
        self
    }

    /// Set a title for the message dialog
    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Show the message dialog
    pub fn show(self) -> Result<()> {
        let mut cmd = Command::new("fzf");
        cmd.arg("--layout").arg("reverse");

        // Add header if we have a title, otherwise use message as header
        if let Some(title) = &self.title {
            cmd.arg("--header")
                .arg(format!("{}\n\n{}", title, self.message));
        } else {
            cmd.arg("--header").arg(&self.message);
        }

        cmd.arg("--prompt").arg("- ");

        // Add confirmation margin arguments for better popup appearance
        for arg in FzfOptions::confirm_margin_args() {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        // Register the process for potential killing if scratchpad becomes invisible
        let pid = child.id();
        let _ = crate::menu::server::register_fzf_process(pid);

        // Write OK text to stdin as the selectable item
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(self.ok_text.as_bytes())?;
        }

        // Wait for the process to complete
        let output = child.wait_with_output()?;

        // Unregister the process since it's done
        crate::menu::server::unregister_fzf_process(pid);

        // Check if fzf was cancelled (exit code 130) or killed (exit code 143)
        if let Some(code) = output.status.code() {
            if code == 130 || code == 143 {
                return Ok(()); // User cancelled, that's fine for a message
            }
        }

        // For message dialog, we don't care about the selection, just display it
        Ok(())
    }
}

/// Helper struct for confirmation dialogs
#[derive(Debug, Clone)]
pub struct ConfirmationItem {
    pub value: ConfirmResult,
    pub text: String,
}

impl FzfSelectable for ConfirmationItem {
    fn fzf_display_text(&self) -> String {
        self.text.clone()
    }

    fn fzf_key(&self) -> String {
        self.text.clone()
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
