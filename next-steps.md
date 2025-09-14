
I want to replace dialoguer with my own fzf wrapper. Here are ideas for the code
and implementation. 

Some changes which still ned to be made:

preview text shuold not be returning `Option<String>` but instead should return
an enum

```
enum FzfPreview {
    Text(somefixedstring)
    Command(shellcommand for preview like for example git branch info)
    None // empty preview for this item
}
```

```rust

use duct::cmd;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;

/// Core trait that types must implement to be selectable with fzf
pub trait FzfSelectable {
    /// The text that appears in the fzf selection list
    fn fzf_display_text(&self) -> String;
    
    /// Optional preview text shown in the preview window
    /// If None, no preview will be shown for this item
    fn fzf_preview_text(&self) -> Option<String> {
        None
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
    pub height: Option<String>,
    pub preview_window: Option<String>, // e.g., "right:50%", "down:40%"
    pub additional_args: Vec<String>,
}

impl Default for FzfOptions {
    fn default() -> Self {
        Self {
            multi_select: false,
            prompt: None,
            height: Some("40%".to_string()),
            preview_window: Some("right:50%".to_string()),
            additional_args: vec![],
        }
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

/// Main fzf wrapper struct
pub struct FzfWrapper {
    options: FzfOptions,
}

/// Internal structure for JSON serialization to the preview script
#[derive(Serialize, Deserialize)]
struct PreviewData {
    key: String,
    preview_text: String,
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
        let mut preview_map: HashMap<String, String> = HashMap::new();
        let mut display_lines = Vec::new();

        for item in items {
            let key = item.fzf_key();
            let display = item.fzf_display_text();
            
            display_lines.push(display.clone());
            item_map.insert(key.clone(), item.clone());
            
            if let Some(preview_text) = item.fzf_preview_text() {
                preview_map.insert(key, preview_text);
            }
        }

        // Create preview script if any items have preview text
        let preview_script = if !preview_map.is_empty() {
            Some(self.create_preview_script(preview_map)?)
        } else {
            None
        };

        // Build fzf command
        let mut fzf_cmd = cmd!("fzf");
        
        if self.options.multi_select {
            fzf_cmd = fzf_cmd.arg("--multi");
        }
        
        if let Some(prompt) = &self.options.prompt {
            fzf_cmd = fzf_cmd.arg("--prompt").arg(prompt);
        }
        
        if let Some(height) = &self.options.height {
            fzf_cmd = fzf_cmd.arg("--height").arg(height);
        }
        
        // Add preview if we have a preview script
        if let Some(ref script_path) = preview_script {
            fzf_cmd = fzf_cmd.arg("--preview");
            fzf_cmd = fzf_cmd.arg(format!("{} {{}}", script_path.display()));
            
            if let Some(preview_window) = &self.options.preview_window {
                fzf_cmd = fzf_cmd.arg("--preview-window").arg(preview_window);
            }
        }
        
        // Add additional arguments
        for arg in &self.options.additional_args {
            fzf_cmd = fzf_cmd.arg(arg);
        }
        
        // Execute fzf
        let input_text = display_lines.join("\n");
        let output = fzf_cmd
            .stdin_bytes(input_text.as_bytes())
            .stdout_capture()
            .stderr_capture()
            .run();

        match output {
            Ok(result) => {
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
                } else {
                    if let Some(item) = item_map.get(selected_lines[0]).cloned() {
                        Ok(FzfResult::Selected(item))
                    } else {
                        Ok(FzfResult::Cancelled)
                    }
                }
            }
            Err(e) => {
                if e.kind() == duct::ErrorKind::Status(Some(130)) {
                    // fzf was cancelled (Ctrl+C)
                    Ok(FzfResult::Cancelled)
                } else {
                    Ok(FzfResult::Error(format!("fzf execution failed: {}", e)))
                }
            }
        }
    }
    
    /// Create a temporary preview script that can handle our preview data
    fn create_preview_script(
        &self,
        preview_map: HashMap<String, String>,
    ) -> Result<NamedTempFile, Box<dyn std::error::Error>> {
        let mut script_file = NamedTempFile::new()?;
        
        // Write a shell script that looks up preview text by key
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
                .map(|(key, preview)| {
                    let escaped_key = key.replace("'", "'\\''");
                    let escaped_preview = preview.replace("'", "'\\''");
                    format!("'{}') echo '{}' ;;", escaped_key, escaped_preview)
                })
                .collect::<Vec<_>>()
                .join("\n")
        );
        
        script_file.write_all(script_content.as_bytes())?;
        script_file.flush()?;
        
        // Make the script executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(script_file.path())?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(script_file.path(), perms)?;
        }
        
        Ok(script_file)
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
    
    fn fzf_preview_text(&self) -> Option<String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .map(|content| {
                if content.len() > 1000 {
                    format!("{}...\n\n[File truncated]", &content[..1000])
                } else {
                    content
                }
            })
            .or_else(|| Some(format!("Binary file or read error: {}", self.path)))
    }
}

#[derive(Debug, Clone)]
pub struct GitBranch {
    pub name: String,
    pub commit_hash: String,
    pub last_commit: String,
}

impl FzfSelectable for GitBranch {
    fn fzf_display_text(&self) -> String {
        self.name.clone()
    }
    
    fn fzf_preview_text(&self) -> Option<String> {
        Some(format!(
            "Branch: {}\nLast commit: {}\nHash: {}",
            self.name, self.last_commit, self.commit_hash
        ))
    }
}

// Usage examples
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn example_usage() {
        let files = vec![
            FileItem {
                path: "src/main.rs".to_string(),
                size: 1024,
                modified: "2024-01-01".to_string(),
            },
            FileItem {
                path: "Cargo.toml".to_string(),
                size: 256,
                modified: "2024-01-02".to_string(),
            },
        ];
        
        // Single selection with preview
        let wrapper = FzfWrapper::with_options(FzfOptions {
            prompt: Some("Select file: ".to_string()),
            preview_window: Some("right:60%".to_string()),
            ..Default::default()
        });
        
        // This would show fzf with file contents in preview
        // let result = wrapper.select(files).unwrap();
    }
}

```
