use anyhow::{Context, Result};
use freedesktop_file_parser::{EntryType, parse};
use std::collections::HashMap;

use crate::launch::types::DesktopAppDetails;

/// Lazy desktop file loader and parser
pub struct DesktopLoader {
    cache: HashMap<String, DesktopAppDetails>,
}

impl DesktopLoader {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Load desktop app details lazily
    pub async fn get_desktop_details(&mut self, desktop_id: &str) -> Result<DesktopAppDetails> {
        // Check if already cached
        if let Some(cached) = self.cache.get(desktop_id) {
            return Ok(cached.clone());
        }

        // Find and parse the desktop file
        let details = self.load_and_parse_desktop_file(desktop_id).await?;
        self.cache.insert(desktop_id.to_string(), details.clone());
        Ok(details)
    }

    /// Find and parse desktop file
    async fn load_and_parse_desktop_file(&self, desktop_id: &str) -> Result<DesktopAppDetails> {
        let file_path = self.find_desktop_file_path(desktop_id).await?;
        let content = std::fs::read_to_string(&file_path).context("Failed to read desktop file")?;
        let desktop_file = parse(&content).context("Failed to parse desktop file")?;

        let (exec, _name, terminal, categories, icon) = match &desktop_file.entry.entry_type {
            EntryType::Application(app) => {
                let exec = app.exec.clone().unwrap_or_default();
                let name = desktop_file.entry.name.default.clone();
                let terminal = app.terminal.unwrap_or(false);
                let categories = app.categories.clone().unwrap_or_default();
                let icon = desktop_file
                    .entry
                    .icon
                    .as_ref()
                    .map(|icon_str| icon_str.content.clone());
                (exec, name, terminal, categories, icon)
            }
            _ => (
                String::new(),
                desktop_id.to_string(),
                false,
                Vec::new(),
                None,
            ), // Fallback for non-application types
        };

        Ok(DesktopAppDetails {
            exec,
            icon,
            categories,
            no_display: desktop_file.entry.no_display.unwrap_or(false),
            terminal,
            file_path,
        })
    }

    /// Find the path to a desktop file by searching XDG directories
    async fn find_desktop_file_path(&self, desktop_id: &str) -> Result<std::path::PathBuf> {
        let data_dirs = self.get_xdg_data_dirs();

        for data_dir in data_dirs {
            let apps_dir = data_dir.join("applications");
            if apps_dir.exists() {
                let desktop_path = apps_dir.join(desktop_id);
                if desktop_path.exists() {
                    return Ok(desktop_path);
                }
            }
        }

        Err(anyhow::anyhow!("Desktop file not found: {}", desktop_id))
    }

    /// Get XDG data directories
    fn get_xdg_data_dirs(&self) -> Vec<std::path::PathBuf> {
        let mut dirs = Vec::new();

        if let Some(home_data) = dirs::data_dir() {
            dirs.push(home_data);
        }

        if let Ok(system_dirs) = std::env::var("XDG_DATA_DIRS") {
            for dir in system_dirs.split(':') {
                if !dir.is_empty() {
                    dirs.push(std::path::PathBuf::from(dir));
                }
            }
        } else {
            dirs.push(std::path::PathBuf::from("/usr/local/share"));
            dirs.push(std::path::PathBuf::from("/usr/share"));
        }

        dirs
    }
}

/// Execute a desktop application
pub fn execute_desktop_app(details: &DesktopAppDetails) -> Result<()> {
    if details.no_display {
        return Err(anyhow::anyhow!("Application is marked as not displayable"));
    }

    // Parse and expand field codes in Exec string
    let exec_cmd = expand_exec_field_codes(&details.exec)?;

    // Split into command and arguments
    let parts: Vec<&str> = exec_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err(anyhow::anyhow!("Empty Exec command"));
    }

    let mut cmd = std::process::Command::new(parts[0]);

    // Add remaining arguments
    for arg in &parts[1..] {
        cmd.arg(arg);
    }

    // Handle terminal execution
    if details.terminal {
        wrap_with_terminal(&mut cmd)?;
    }

    // Execute in background
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch desktop app: {}", e))?;

    Ok(())
}

/// Expand field codes in Exec string
fn expand_exec_field_codes(exec: &str) -> Result<String> {
    let mut expanded = exec.to_string();

    // Handle %% -> %
    expanded = expanded.replace("%%", "%");

    // Handle %c -> application name (not available in context, so remove)
    expanded = expanded.replace("%c", "");

    // Handle %f, %F, %u, %U (file arguments - not supported in launcher, remove)
    expanded = expanded.replace("%f", "");
    expanded = expanded.replace("%F", "");
    expanded = expanded.replace("%u", "");
    expanded = expanded.replace("%U", "");

    // Handle %i (icon name - not supported, remove)
    expanded = expanded.replace("%i", "");

    // Handle %k (desktop file path - not supported, remove)
    expanded = expanded.replace("%k", "");

    // Clean up multiple spaces that might result from removing field codes
    while expanded.contains("  ") {
        expanded = expanded.replace("  ", " ");
    }
    expanded = expanded.trim().to_string();

    Ok(expanded)
}

/// Wrap command with terminal
fn wrap_with_terminal(cmd: &mut std::process::Command) -> Result<()> {
    crate::common::terminal::wrap_with_terminal(cmd)
}
