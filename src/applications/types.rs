/// Lightweight enum containing only display name and identifier
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchItem {
    DesktopApp {
        id: String,
        name: String,
        path: std::path::PathBuf,
    },
    PathExecutable {
        name: String,
        display_name: String,
    },
}

/// Desktop app details loaded lazily when needed for execution
#[derive(Debug, Clone, Default)]
pub struct DesktopAppDetails {
    pub exec: String,
    pub name: String,
    pub icon: Option<String>,
    pub desktop_path: std::path::PathBuf,
    pub no_display: bool,
    pub terminal: bool,
}

impl LaunchItem {
    pub fn sort_key(&self) -> String {
        self.to_string().to_lowercase()
    }

    pub fn metadata_type(&self) -> &'static str {
        match self {
            LaunchItem::DesktopApp { .. } => "desktop",
            LaunchItem::PathExecutable { .. } => "path",
        }
    }

    pub fn stable_key(&self) -> String {
        match self {
            LaunchItem::DesktopApp { id, .. } => format!("desktop:{id}"),
            LaunchItem::PathExecutable { name, .. } => format!("path:{name}"),
        }
    }

    pub fn identifier(&self) -> &str {
        match self {
            LaunchItem::DesktopApp { id, .. } => id,
            LaunchItem::PathExecutable { name, .. } => name,
        }
    }
}

impl std::fmt::Display for LaunchItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            LaunchItem::DesktopApp { name, .. } => name,
            LaunchItem::PathExecutable { display_name, .. } => display_name,
        };
        write!(f, "{}", name)
    }
}
