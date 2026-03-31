//! Wine prefix scanner: reverse-lookup saves from Ludusavi manifest

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::manifest::load_manifest;
use super::types::{DiscoveredWineSave, FileConstraint};

/// Placeholder substitution context for a wine prefix
struct WinePrefixContext {
    prefix: PathBuf,
    users: Vec<String>,
}

impl WinePrefixContext {
    fn new(prefix: &Path) -> Result<Self> {
        let drive_c = prefix.join("drive_c");
        let users_dir = drive_c.join("users");

        let users = if users_dir.is_dir() {
            std::fs::read_dir(&users_dir)
                .ok()
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                        .filter_map(|e| {
                            let name = e.file_name();
                            let name_str = name.to_string_lossy();
                            // Skip special directories
                            if name_str == "Public" || name_str == "All Users" {
                                None
                            } else {
                                Some(name_str.to_string())
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(Self {
            prefix: prefix.to_path_buf(),
            users,
        })
    }

    /// Substitute placeholders for a given path pattern
    /// Returns all expanded paths (one per user for user-specific placeholders)
    fn expand_paths(&self, pattern: &str) -> Vec<String> {
        let mut results = Vec::new();

        // Check if pattern uses user-specific placeholders
        let has_user_placeholder = pattern.contains("<winAppData>")
            || pattern.contains("<winLocalAppData>")
            || pattern.contains("<winLocalAppDataLow>")
            || pattern.contains("<winDocuments>")
            || pattern.contains("<osUserName>");

        if has_user_placeholder {
            if self.users.is_empty() {
                // No users found, skip
                return results;
            }

            for user in &self.users {
                let mut expanded = pattern.to_string();
                expanded = expanded.replace("<winAppData>", &self.win_app_data(user));
                expanded = expanded.replace("<winLocalAppData>", &self.win_local_app_data(user));
                expanded =
                    expanded.replace("<winLocalAppDataLow>", &self.win_local_app_data_low(user));
                expanded = expanded.replace("<winDocuments>", &self.win_documents(user));
                expanded = expanded.replace("<osUserName>", user);
                expanded = expanded.replace("<home>", &self.home_dir());
                expanded = expanded.replace("<winProgramData>", &self.win_program_data());
                expanded = expanded.replace("<winDir>", &self.win_dir());
                expanded = expanded.replace("<xdgData>", &self.xdg_data());
                expanded = expanded.replace("<xdgConfig>", &self.xdg_config());
                results.push(expanded);
            }
        } else {
            // Non-user-specific placeholders
            let mut expanded = pattern.to_string();
            expanded = expanded.replace("<home>", &self.home_dir());
            expanded = expanded.replace("<winProgramData>", &self.win_program_data());
            expanded = expanded.replace("<winDir>", &self.win_dir());
            expanded = expanded.replace("<xdgData>", &self.xdg_data());
            expanded = expanded.replace("<xdgConfig>", &self.xdg_config());
            results.push(expanded);
        }

        results
    }

    fn win_app_data(&self, user: &str) -> String {
        self.prefix
            .join("drive_c")
            .join("users")
            .join(user)
            .join("AppData")
            .join("Roaming")
            .to_string_lossy()
            .to_string()
    }

    fn win_local_app_data(&self, user: &str) -> String {
        self.prefix
            .join("drive_c")
            .join("users")
            .join(user)
            .join("AppData")
            .join("Local")
            .to_string_lossy()
            .to_string()
    }

    fn win_local_app_data_low(&self, user: &str) -> String {
        self.prefix
            .join("drive_c")
            .join("users")
            .join(user)
            .join("AppData")
            .join("LocalLow")
            .to_string_lossy()
            .to_string()
    }

    fn win_documents(&self, user: &str) -> String {
        self.prefix
            .join("drive_c")
            .join("users")
            .join(user)
            .join("Documents")
            .to_string_lossy()
            .to_string()
    }

    fn win_program_data(&self) -> String {
        self.prefix
            .join("drive_c")
            .join("ProgramData")
            .to_string_lossy()
            .to_string()
    }

    fn win_dir(&self) -> String {
        self.prefix
            .join("drive_c")
            .join("Windows")
            .to_string_lossy()
            .to_string()
    }

    fn home_dir(&self) -> String {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn xdg_data(&self) -> String {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .or_else(|| dirs::data_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| format!("{}/.local/share", self.home_dir()))
    }

    fn xdg_config(&self) -> String {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .or_else(|| dirs::config_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| format!("{}/.config", self.home_dir()))
    }
}

/// Check if a file constraint is Windows-relevant
fn is_windows_constraint(constraints: &[FileConstraint]) -> bool {
    if constraints.is_empty() {
        // No constraints means it applies everywhere
        return true;
    }
    constraints.iter().any(|c| {
        c.os.as_ref().map(|os| {
            let os_lower = os.to_lowercase();
            os_lower == "windows" || os_lower == "win"
        }) == Some(true)
    })
}

/// Check if a path pattern matches any existing paths (glob evaluation)
fn path_exists(pattern: &str) -> bool {
    // Handle glob patterns
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        if let Ok(pattern_obj) = Pattern::new(pattern) {
            // For glob patterns, check if any path matches
            // We only check the directory portion for existence
            let base = extract_base_path(pattern);
            return Path::new(&base).exists();
        }
        return false;
    }

    // Direct path check
    Path::new(pattern).exists()
}

/// Extract the base (non-glob) portion of a path pattern
fn extract_base_path(pattern: &str) -> String {
    let glob_chars = ['*', '?', '['];
    let mut result = pattern.to_string();

    // Find the first glob character and truncate there
    for &ch in &glob_chars {
        if let Some(pos) = result.find(ch) {
            result.truncate(pos);
            break;
        }
    }

    // Remove trailing separators
    while result.ends_with('/') || result.ends_with('\\') {
        result.pop();
    }

    result
}

/// Scan a wine prefix for Ludusavi-compatible save games
pub fn scan_wine_prefix(prefix: &Path) -> Result<Vec<DiscoveredWineSave>> {
    let manifest = load_manifest()?;
    let ctx = WinePrefixContext::new(prefix)?;

    let mut results = Vec::new();

    for (game_name, entry) in &manifest {
        // Skip aliases
        if entry.alias.is_some() {
            continue;
        }

        // Skip entries with no files
        if entry.files.is_empty() {
            continue;
        }

        let mut matched_paths = Vec::new();

        for (file_pattern, file_entry) in &entry.files {
            // Only consider Windows entries
            if !is_windows_constraint(&file_entry.when) {
                continue;
            }

            // Expand placeholders
            let expanded = ctx.expand_paths(file_pattern);

            for expanded_path in expanded {
                if path_exists(&expanded_path) {
                    matched_paths.push((expanded_path, file_entry.tags.clone()));
                }
            }
        }

        // If any paths matched, add to results
        if !matched_paths.is_empty() {
            for (save_path, tags) in matched_paths {
                results.push(DiscoveredWineSave::new(game_name.clone(), save_path, tags));
            }
        }
    }

    // Deduplicate by game_name + save_path
    results.sort_by(|a, b| {
        a.game_name
            .cmp(&b.game_name)
            .then_with(|| a.save_path.cmp(&b.save_path))
    });
    results.dedup_by(|a, b| a.game_name == b.game_name && a.save_path == b.save_path);

    Ok(results)
}
