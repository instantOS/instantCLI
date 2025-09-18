use anyhow::{Context, Result};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::launch::types::DesktopApp;

/// Desktop file discovery for XDG applications
pub struct DesktopDiscovery {
    pub data_dirs: Vec<PathBuf>,
}

impl DesktopDiscovery {
    /// Create a new desktop discovery instance
    pub fn new() -> Result<Self> {
        let data_dirs = Self::get_xdg_data_dirs();
        Ok(Self { data_dirs })
    }

    /// Get XDG data directories for desktop file discovery
    fn get_xdg_data_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // Add user-specific data directory
        if let Some(data_home) = dirs::data_dir() {
            dirs.push(data_home);
        }

        // Add system data directories
        if let Ok(data_dirs) = env::var("XDG_DATA_DIRS") {
            for dir in data_dirs.split(':') {
                if !dir.is_empty() {
                    dirs.push(PathBuf::from(dir));
                }
            }
        } else {
            // Default XDG data directories
            dirs.push(PathBuf::from("/usr/local/share"));
            dirs.push(PathBuf::from("/usr/share"));
        }

        // Remove duplicates while preserving order
        let mut seen = HashSet::new();
        dirs.retain(|dir| {
            let path_str = dir.to_string_lossy().to_string();
            seen.insert(path_str)
        });

        dirs
    }

    /// Discover all desktop applications
    pub fn discover_applications(&self) -> Result<Vec<DesktopApp>> {
        let mut apps = Vec::new();

        for data_dir in &self.data_dirs {
            let apps_dir = data_dir.join("applications");
            if apps_dir.exists() {
                let dir_apps = self.scan_directory(&apps_dir)?;
                apps.extend(dir_apps);
            }
        }

        // Sort by name for consistent ordering with cached lowercase names
        apps.sort_by_cached_key(|app| app.name.to_lowercase());

        Ok(apps)
    }

    /// Scan a directory for desktop files
    fn scan_directory(&self, dir: &Path) -> Result<Vec<DesktopApp>> {
        let mut apps = Vec::new();

        for entry in WalkDir::new(dir)
            .max_depth(10)
            .into_iter()
            .filter_entry(|e| {
                // Skip hidden directories and files
                !e.file_name().to_string_lossy().starts_with('.')
            })
        {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                if let Ok(app) = self.parse_desktop_file(path) {
                    if app.should_display() {
                        apps.push(app);
                    }
                }
            }
        }

        Ok(apps)
    }

    /// Parse a desktop file
    fn parse_desktop_file(&self, path: &Path) -> Result<DesktopApp> {
        let content = fs::read_to_string(path)
            .context(format!("Failed to read desktop file: {}", path.display()))?;

        DesktopApp::from_content(&content, path.to_path_buf())
    }

    /// Get desktop applications by category
    pub fn get_apps_by_category(&self, category: &str) -> Result<Vec<DesktopApp>> {
        let apps = self.discover_applications()?;
        let filtered: Vec<DesktopApp> = apps
            .into_iter()
            .filter(|app| {
                app.categories
                    .iter()
                    .any(|c| c.to_lowercase() == category.to_lowercase())
            })
            .collect();

        Ok(filtered)
    }

    /// Search desktop applications by name
    pub fn search_apps(&self, query: &str) -> Result<Vec<DesktopApp>> {
        let apps = self.discover_applications()?;
        let query_lower = query.to_lowercase();

        let filtered: Vec<DesktopApp> = apps
            .into_iter()
            .filter(|app| {
                app.name.to_lowercase().contains(&query_lower)
                    || app.desktop_id.to_lowercase().contains(&query_lower)
                    || app
                        .categories
                        .iter()
                        .any(|c| c.to_lowercase().contains(&query_lower))
            })
            .collect();

        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_xdg_data_dirs() {
        let dirs = DesktopDiscovery::get_xdg_data_dirs();
        assert!(!dirs.is_empty());

        // Should contain at least /usr/share/applications
        let has_usr_share = dirs
            .iter()
            .any(|dir| dir.to_string_lossy().contains("/usr/share"));
        assert!(has_usr_share);
    }

    #[test]
    fn test_parse_desktop_file() {
        let content = r#"[Desktop Entry]
Type=Application
Name=Test App
Exec=test-app %f
Icon=test-icon
Categories=Utility;System;
Terminal=false
"#;

        let temp_dir = TempDir::new().unwrap();
        let desktop_path = temp_dir.path().join("test-app.desktop");
        fs::write(&desktop_path, content).unwrap();

        let discovery = DesktopDiscovery::new().unwrap();
        let app = discovery.parse_desktop_file(&desktop_path).unwrap();

        assert_eq!(app.name, "Test App");
        assert_eq!(app.exec, "test-app %f");
        assert_eq!(app.icon, Some("test-icon".to_string()));
        assert_eq!(app.categories, vec!["Utility", "System"]);
        assert!(!app.terminal);
        assert!(!app.no_display);
    }

    #[test]
    fn test_exec_field_expansion() {
        let content = r#"[Desktop Entry]
Type=Application
Name=Test App
Exec=test-app %f %c
Terminal=false
"#;

        let temp_dir = TempDir::new().unwrap();
        let desktop_path = temp_dir.path().join("test-app.desktop");
        fs::write(&desktop_path, content).unwrap();

        let discovery = DesktopDiscovery::new().unwrap();
        let app = discovery.parse_desktop_file(&desktop_path).unwrap();

        let expanded = app.expand_exec(&["file.txt".to_string()]);
        assert_eq!(expanded, "test-app Test App file.txt");
    }
}
