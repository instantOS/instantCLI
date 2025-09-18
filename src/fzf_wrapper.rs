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
    pub additional_args: Vec<String>,
}

impl Default for FzfOptions {
    fn default() -> Self {
        Self {
            multi_select: false,
            prompt: None,
            additional_args: Self::default_margin_args(),
        }
    }
}

impl FzfOptions {
    /// Default margin arguments used across the application
    fn default_margin_args() -> Vec<String> {
        vec![
            "--margin".to_string(),
            "10%,2%".to_string(), // 10% vertical, 2% horizontal
            "--min-height".to_string(),
            "10".to_string(),
        ]
    }

    /// Margin arguments for input dialogs (larger vertical margin)
    fn input_margin_args() -> Vec<String> {
        vec![
            "--margin".to_string(),
            "20%,2%".to_string(), // 20% vertical, 2% horizontal
            "--min-height".to_string(),
            "10".to_string(),
        ]
    }

    /// Margin arguments for confirmation dialogs (largest vertical margin)
    fn confirm_margin_args() -> Vec<String> {
        vec![
            "--margin".to_string(),
            "40%,2%".to_string(), // 40% vertical, 2% horizontal
            "--min-height".to_string(),
            "10".to_string(),
        ]
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
    ) -> Result<(TempPath, std::path::PathBuf), Box<dyn std::error::Error>> {
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

    /// Select from a vector of FzfSelectable items
    pub fn select<T: FzfSelectable + Clone>(
        &self,
        items: Vec<T>,
    ) -> Result<FzfResult<T>, Box<dyn std::error::Error>> {
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

        if self.options.multi_select {
            cmd.arg("--multi");
        }

        if let Some(prompt) = &self.options.prompt {
            //TODO: add a " > " to the end of the prompt for spacing
            cmd.arg("--prompt").arg(prompt);
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

        // Execute fzf
        let input_text = display_lines.join("\n");
        let output = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    use std::io::Write;
                    stdin.write_all(input_text.as_bytes())?;
                }
                child.wait_with_output()
            });

        // The preview script is kept alive by _preview_script_keeper
        // which will be dropped when this function ends, after fzf has finished

        match output {
            Ok(result) => {
                // Check if fzf was cancelled (exit code 130)
                if let Some(130) = result.status.code() {
                    return Ok(FzfResult::Cancelled);
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
    pub fn select_one<T: FzfSelectable + Clone>(
        items: Vec<T>,
    ) -> Result<Option<T>, Box<dyn std::error::Error>> {
        let wrapper = FzfWrapper::new();
        match wrapper.select(items)? {
            FzfResult::Selected(item) => Ok(Some(item)),
            _ => Ok(None),
        }
    }

    /// Quick multi-selection with default options
    pub fn select_many<T: FzfSelectable + Clone>(
        items: Vec<T>,
    ) -> Result<Vec<T>, Box<dyn std::error::Error>> {
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
    pub fn input(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut cmd = Command::new("fzf");
        cmd.arg("--print-query")
            .arg("--no-info")
            .arg("--prompt")
            .arg(format!("{prompt} "));

        // Add input-specific margin arguments
        for arg in FzfOptions::input_margin_args() {
            cmd.arg(arg);
        }

        let output = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(b"")?;
                }
                child.wait_with_output()
            })?;

        // Check if fzf was cancelled (exit code 130)
        if let Some(130) = output.status.code() {
            return Ok(String::new());
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

    /// Confirmation dialog with yes/no options
    pub fn confirm(message: &str) -> Result<ConfirmResult, Box<dyn std::error::Error>> {
        let items = vec![
            ConfirmationItem {
                value: ConfirmResult::Yes,
                text: "Yes".to_string(),
            },
            ConfirmationItem {
                value: ConfirmResult::No,
                text: "No".to_string(),
            },
        ];

        let wrapper = FzfWrapper::with_options(FzfOptions {
            prompt: Some(format!("{message} [Y/n]: ")),
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
