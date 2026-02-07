use std::path::PathBuf;

/// Lightweight enum containing only display name and identifier
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchItem {
    DesktopApp(String),     // desktop_id (e.g., "firefox.desktop")
    PathExecutable(String), // executable name
}

/// Desktop app details loaded lazily when needed for execution
#[derive(Debug, Clone, Default)]
pub struct DesktopAppDetails {
    pub exec: String,            // Exec command with field codes
    pub icon: Option<String>,    // Icon name
    pub categories: Vec<String>, // Application categories
    pub no_display: bool,        // Should be hidden
    pub terminal: bool,          // Run in terminal
    pub file_path: PathBuf,      // Path to .desktop file
}

impl LaunchItem {
    pub fn sort_key(&self) -> String {
        self.to_string().to_lowercase()
    }

    pub fn metadata_type(&self) -> &'static str {
        match self {
            LaunchItem::DesktopApp(_) => "desktop",
            LaunchItem::PathExecutable(_) => "path",
        }
    }

    pub fn metadata_key(&self) -> String {
        match self {
            LaunchItem::DesktopApp(id) => format!("desktop:{id}"),
            LaunchItem::PathExecutable(name) => {
                if name.starts_with("path:") {
                    name.clone()
                } else {
                    format!("path:{name}")
                }
            }
        }
    }
}

impl std::fmt::Display for LaunchItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            LaunchItem::DesktopApp(id) => {
                // Extract name from desktop_id (remove .desktop suffix)
                id.strip_suffix(".desktop").unwrap_or(id)
            }
            LaunchItem::PathExecutable(name) => name,
        };
        write!(f, "{}", name)
    }
}
