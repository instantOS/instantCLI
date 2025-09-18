use anyhow::Result;
use freedesktop_file_parser::{EntryType, parse};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Launch item that can be either a desktop application or a PATH executable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LaunchItem {
    DesktopApp(DesktopApp),
    PathExecutable(String),
}

impl LaunchItem {
    /// Get the display name for the launch item
    pub fn display_name(&self) -> String {
        match self {
            LaunchItem::DesktopApp(app) => app.name.clone(),
            LaunchItem::PathExecutable(name) => name.clone(),
        }
    }

    /// Get the display name with potential path: prefix for conflict resolution
    pub fn display_name_with_prefix(&self, has_conflict: bool) -> String {
        match self {
            LaunchItem::DesktopApp(app) => app.name.clone(),
            LaunchItem::PathExecutable(name) => {
                if has_conflict {
                    format!("path:{}", name)
                } else {
                    name.clone()
                }
            }
        }
    }

    /// Check if this is a desktop application
    pub fn is_desktop_app(&self) -> bool {
        matches!(self, LaunchItem::DesktopApp(_))
    }

    /// Check if this is a PATH executable
    pub fn is_path_executable(&self) -> bool {
        matches!(self, LaunchItem::PathExecutable(_))
    }
}

/// Desktop application parsed from a .desktop file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopApp {
    pub desktop_id: String,
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub no_display: bool,
    pub terminal: bool,
    pub file_path: PathBuf,
}

impl DesktopApp {
    /// Parse a desktop file from its content
    pub fn from_content(content: &str, file_path: PathBuf) -> Result<Self> {
        let desktop_file = parse(content)?;

        let name = desktop_file.entry.name.default.clone();
        let (exec, terminal) = match &desktop_file.entry.entry_type {
            EntryType::Application(app) => (
                app.exec.clone().unwrap_or_default(),
                app.terminal.unwrap_or(false),
            ),
            _ => return Err(anyhow::anyhow!("Desktop file is not an application")),
        };

        let icon = desktop_file.entry.icon.map(|icon| icon.content);
        let categories = Vec::new(); // Categories not available in this version of the parser
        let no_display = desktop_file.entry.no_display.unwrap_or(false);

        let desktop_id = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(Self {
            desktop_id,
            name,
            exec,
            icon,
            categories,
            no_display,
            terminal,
            file_path,
        })
    }

    /// Check if the desktop application should be displayed
    pub fn should_display(&self) -> bool {
        !self.no_display
    }

    /// Expand field codes in the Exec string
    pub fn expand_exec(&self, args: &[String]) -> String {
        let mut exec = self.exec.clone();

        // Simple field code expansion for basic use cases
        // %f, %F, %u, %U - we'll just remove these for now
        exec = exec.replace("%f", "");
        exec = exec.replace("%F", "");
        exec = exec.replace("%u", "");
        exec = exec.replace("%U", "");

        // %i - icon name (we don't use this in simple execution)
        exec = exec.replace("%i", "");

        // %c - application name
        exec = exec.replace("%c", &self.name);

        // %k - desktop file path
        exec = exec.replace("%k", &self.file_path.display().to_string());

        // %% - literal percent
        exec = exec.replace("%%", "%");

        // Add provided arguments if any
        if !args.is_empty() {
            exec.push(' ');
            exec.push_str(&args.join(" "));
        }

        exec.trim().to_string()
    }
}

/// Launch item with metadata for display and frecency tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchItemWithMetadata {
    pub item: LaunchItem,
    pub frecency_score: f64,
    pub launch_count: u32,
}

impl LaunchItemWithMetadata {
    pub fn new(item: LaunchItem) -> Self {
        Self {
            item,
            frecency_score: 0.0,
            launch_count: 0,
        }
    }

    pub fn with_frecency(item: LaunchItem, frecency_score: f64, launch_count: u32) -> Self {
        Self {
            item,
            frecency_score,
            launch_count,
        }
    }
}
