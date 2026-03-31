//! Wine prefix scanner: reverse-lookup saves from Ludusavi manifest

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::Result;

use super::manifest::load_manifest;
use super::types::{DiscoveredWineSave, FileConstraint, LudusaviManifest};

static WINDOWS_MANIFEST: OnceLock<std::result::Result<Vec<WindowsGameEntry>, String>> =
    OnceLock::new();

#[derive(Debug, Clone)]
struct WindowsGameEntry {
    game_name: String,
    files: Vec<WindowsFileEntry>,
}

#[derive(Debug, Clone)]
struct WindowsFileEntry {
    pattern: String,
    tags: Vec<String>,
    needs_user: bool,
}

#[derive(Debug, Clone)]
struct UserPaths {
    name: String,
    win_home: String,
    win_app_data: String,
    win_local_app_data: String,
    win_local_app_data_low: String,
    win_documents: String,
}

/// Placeholder substitution context for a wine prefix
struct WinePrefixContext {
    users: Vec<UserPaths>,
    home_dir: String,
    xdg_data: String,
    xdg_config: String,
    win_program_data: String,
    win_dir: String,
}

impl WinePrefixContext {
    fn new(prefix: &Path) -> Self {
        let drive_c = prefix.join("drive_c");
        let users_dir = drive_c.join("users");
        let home_dir = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let xdg_data = std::env::var("XDG_DATA_HOME")
            .ok()
            .or_else(|| dirs::data_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| format!("{home_dir}/.local/share"));
        let xdg_config = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .or_else(|| dirs::config_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| format!("{home_dir}/.config"));

        let users = if users_dir.is_dir() {
            std::fs::read_dir(&users_dir)
                .ok()
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                        .filter_map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            if name == "Public" || name == "All Users" {
                                return None;
                            }
                            let user_root = drive_c.join("users").join(&name);
                            Some(UserPaths {
                                name,
                                win_home: user_root.to_string_lossy().to_string(),
                                win_app_data: user_root
                                    .join("AppData")
                                    .join("Roaming")
                                    .to_string_lossy()
                                    .to_string(),
                                win_local_app_data: user_root
                                    .join("AppData")
                                    .join("Local")
                                    .to_string_lossy()
                                    .to_string(),
                                win_local_app_data_low: user_root
                                    .join("AppData")
                                    .join("LocalLow")
                                    .to_string_lossy()
                                    .to_string(),
                                win_documents: user_root
                                    .join("Documents")
                                    .to_string_lossy()
                                    .to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Self {
            users,
            home_dir,
            xdg_data,
            xdg_config,
            win_program_data: drive_c.join("ProgramData").to_string_lossy().to_string(),
            win_dir: drive_c.join("Windows").to_string_lossy().to_string(),
        }
    }

    /// Substitute placeholders for a given path pattern.
    fn expand_paths(&self, entry: &WindowsFileEntry) -> Vec<String> {
        if entry.needs_user {
            if self.users.is_empty() {
                return Vec::new();
            }

            self.users
                .iter()
                .map(|user| self.expand_pattern_for_user(&entry.pattern, Some(user)))
                .collect()
        } else {
            vec![self.expand_pattern_for_user(&entry.pattern, None)]
        }
    }

    fn expand_pattern_for_user(&self, pattern: &str, user: Option<&UserPaths>) -> String {
        let mut expanded = pattern.to_string();

        if let Some(user) = user {
            expanded = expanded.replace("<home>", &user.win_home);
            expanded = expanded.replace("<winAppData>", &user.win_app_data);
            expanded = expanded.replace("<winLocalAppData>", &user.win_local_app_data);
            expanded = expanded.replace("<winLocalAppDataLow>", &user.win_local_app_data_low);
            expanded = expanded.replace("<winDocuments>", &user.win_documents);
            expanded = expanded.replace("<osUserName>", &user.name);
        }

        expanded = expanded.replace("<home>", &self.home_dir);
        expanded = expanded.replace("<winProgramData>", &self.win_program_data);
        expanded = expanded.replace("<winDir>", &self.win_dir);
        expanded = expanded.replace("<xdgData>", &self.xdg_data);
        expanded = expanded.replace("<xdgConfig>", &self.xdg_config);
        expanded
    }
}

fn load_windows_manifest() -> Result<&'static [WindowsGameEntry]> {
    let result = WINDOWS_MANIFEST.get_or_init(|| match load_manifest() {
        Ok(manifest) => Ok(build_windows_manifest(manifest)),
        Err(err) => Err(err.to_string()),
    });

    match result {
        Ok(entries) => Ok(entries.as_slice()),
        Err(err) => Err(anyhow::anyhow!("Failed to load Ludusavi manifest: {}", err)),
    }
}

fn build_windows_manifest(manifest: LudusaviManifest) -> Vec<WindowsGameEntry> {
    let mut entries = Vec::new();

    for (game_name, entry) in manifest {
        if entry.alias.is_some() || entry.files.is_empty() {
            continue;
        }

        let files: Vec<WindowsFileEntry> = entry
            .files
            .into_iter()
            .filter(|(_, file_entry)| is_windows_constraint(&file_entry.when))
            .map(|(pattern, file_entry)| WindowsFileEntry {
                needs_user: pattern_uses_user_placeholders(&pattern),
                pattern,
                tags: file_entry.tags,
            })
            .collect();

        if !files.is_empty() {
            entries.push(WindowsGameEntry { game_name, files });
        }
    }

    entries
}

fn pattern_uses_user_placeholders(pattern: &str) -> bool {
    pattern.contains("<home>")
        || pattern.contains("<winAppData>")
        || pattern.contains("<winLocalAppData>")
        || pattern.contains("<winLocalAppDataLow>")
        || pattern.contains("<winDocuments>")
        || pattern.contains("<osUserName>")
}

/// Check if a file constraint is Windows-relevant
fn is_windows_constraint(constraints: &[FileConstraint]) -> bool {
    if constraints.is_empty() {
        return true;
    }
    constraints.iter().any(|c| {
        c.os.as_ref().map(|os| {
            let os_lower = os.to_lowercase();
            os_lower == "windows" || os_lower == "win"
        }) == Some(true)
    })
}

#[derive(Default)]
struct PathExistenceCache {
    entries: HashMap<std::path::PathBuf, bool>,
}

impl PathExistenceCache {
    fn exists(&mut self, pattern: &str) -> bool {
        let probe_path = normalize_probe_path(pattern);
        self.exists_path(&probe_path)
    }

    fn exists_path(&mut self, path: &Path) -> bool {
        if path.as_os_str().is_empty() {
            return false;
        }

        if let Some(&exists) = self.entries.get(path) {
            return exists;
        }

        let mut unresolved = Vec::new();
        let mut current = Some(path);

        while let Some(candidate) = current {
            if let Some(&exists) = self.entries.get(candidate) {
                if exists {
                    break;
                }

                for unresolved_path in unresolved {
                    self.entries.insert(unresolved_path, false);
                }
                return false;
            }

            unresolved.push(candidate.to_path_buf());
            current = candidate.parent();
        }

        for (index, candidate) in unresolved.iter().enumerate().rev() {
            let exists = candidate.exists();
            self.entries.insert(candidate.clone(), exists);

            if !exists {
                for descendant in unresolved.iter().take(index) {
                    self.entries.insert(descendant.clone(), false);
                }
                return false;
            }
        }

        let path_exists = path.exists();
        self.entries.insert(path.to_path_buf(), path_exists);
        path_exists
    }
}

/// Check if a path pattern matches any existing paths (glob evaluation)
fn path_exists(pattern: &str, cache: &mut PathExistenceCache) -> bool {
    cache.exists(pattern)
}

fn normalize_probe_path(pattern: &str) -> std::path::PathBuf {
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        let base = extract_base_path(pattern);
        return std::path::PathBuf::from(base);
    }

    std::path::PathBuf::from(pattern)
}

/// Extract the base (non-glob) portion of a path pattern
fn extract_base_path(pattern: &str) -> String {
    let glob_chars = ['*', '?', '['];
    let mut result = pattern.to_string();

    for &ch in &glob_chars {
        if let Some(pos) = result.find(ch) {
            result.truncate(pos);
            break;
        }
    }

    while result.ends_with('/') || result.ends_with('\\') {
        result.pop();
    }

    result
}

/// Scan a wine prefix for Ludusavi-compatible save games
pub fn scan_wine_prefix(prefix: &Path) -> Result<Vec<DiscoveredWineSave>> {
    let manifest = load_windows_manifest()?;
    let ctx = WinePrefixContext::new(prefix);
    let mut path_cache = PathExistenceCache::default();

    let mut results = Vec::new();

    for entry in manifest {
        for file in &entry.files {
            for expanded_path in ctx.expand_paths(file) {
                if path_exists(&expanded_path, &mut path_cache) {
                    results.push(DiscoveredWineSave::new(
                        entry.game_name.clone(),
                        expanded_path,
                        file.tags.clone(),
                    ));
                }
            }
        }
    }

    results.sort_by(|a, b| {
        a.game_name
            .cmp(&b.game_name)
            .then_with(|| a.save_path.cmp(&b.save_path))
    });
    results.dedup_by(|a, b| a.game_name == b.game_name && a.save_path == b.save_path);

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::game::platforms::ludusavi::types::{FileEntry, GameEntry};

    #[test]
    fn build_windows_manifest_filters_aliases_and_non_windows_entries() {
        let mut manifest = HashMap::new();
        manifest.insert(
            "Keep Me".to_string(),
            GameEntry {
                alias: None,
                files: HashMap::from([
                    (
                        "<winDocuments>/Keep".to_string(),
                        FileEntry {
                            tags: vec!["save".to_string()],
                            when: vec![FileConstraint {
                                os: Some("windows".to_string()),
                                store: None,
                            }],
                        },
                    ),
                    (
                        "/tmp/linux".to_string(),
                        FileEntry {
                            tags: vec![],
                            when: vec![FileConstraint {
                                os: Some("linux".to_string()),
                                store: None,
                            }],
                        },
                    ),
                ]),
            },
        );
        manifest.insert(
            "Alias".to_string(),
            GameEntry {
                alias: Some("Other".to_string()),
                files: HashMap::new(),
            },
        );

        let filtered = build_windows_manifest(manifest);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].game_name, "Keep Me");
        assert_eq!(filtered[0].files.len(), 1);
        assert!(filtered[0].files[0].needs_user);
    }

    #[test]
    fn context_expands_without_recomputing_global_paths() {
        let prefix = tempfile::tempdir().unwrap();
        let ctx = WinePrefixContext::new(prefix.path());
        let entry = WindowsFileEntry {
            pattern: "<home>/foo/<xdgConfig>".to_string(),
            tags: vec![],
            needs_user: false,
        };

        let expanded = ctx.expand_paths(&entry);
        assert_eq!(expanded.len(), 1);
        assert!(expanded[0].contains("/foo/"));
    }

    #[test]
    fn home_placeholder_uses_wine_user_home() {
        let prefix = tempfile::tempdir().unwrap();
        let user_root = prefix
            .path()
            .join("drive_c")
            .join("users")
            .join("steamuser");
        std::fs::create_dir_all(&user_root).unwrap();

        let ctx = WinePrefixContext::new(prefix.path());
        let entry = WindowsFileEntry {
            pattern: "<home>/AppData/LocalLow/Game".to_string(),
            tags: vec![],
            needs_user: pattern_uses_user_placeholders("<home>/AppData/LocalLow/Game"),
        };

        let expanded = ctx.expand_paths(&entry);
        assert_eq!(expanded.len(), 1);
        assert_eq!(
            expanded[0],
            user_root
                .join("AppData")
                .join("LocalLow")
                .join("Game")
                .display()
                .to_string()
        );
    }

    #[test]
    fn normalize_probe_path_uses_non_glob_base() {
        let path = normalize_probe_path("/tmp/foo/bar/*.sav");
        assert_eq!(path, Path::new("/tmp/foo/bar"));
    }

    #[test]
    fn missing_ancestor_marks_descendants_missing() {
        let temp = tempfile::tempdir().unwrap();
        let missing_parent = temp.path().join("missing");
        let descendant = missing_parent.join("child").join("save.dat");

        let mut cache = PathExistenceCache::default();
        assert!(!cache.exists_path(&descendant));
        assert_eq!(cache.entries.get(&missing_parent), Some(&false));
        assert_eq!(cache.entries.get(&descendant), Some(&false));
    }

    #[test]
    fn existing_ancestor_allows_descendant_probe() {
        let temp = tempfile::tempdir().unwrap();
        let existing_parent = temp.path().join("existing");
        let missing_child = existing_parent.join("child").join("save.dat");
        std::fs::create_dir_all(&existing_parent).unwrap();

        let mut cache = PathExistenceCache::default();
        assert!(!cache.exists_path(&missing_child));
        assert_eq!(cache.entries.get(&existing_parent), Some(&true));
        assert_eq!(cache.entries.get(&missing_child), Some(&false));
    }

    #[test]
    fn cached_existing_ancestor_does_not_make_missing_descendant_exist() {
        let temp = tempfile::tempdir().unwrap();
        let existing_parent = temp.path().join("existing");
        let missing_child = existing_parent.join("child").join("save.dat");
        std::fs::create_dir_all(&existing_parent).unwrap();

        let mut cache = PathExistenceCache::default();
        assert!(cache.exists_path(&existing_parent));
        assert!(!cache.exists_path(&missing_child));
        assert_eq!(cache.entries.get(&existing_parent), Some(&true));
        assert_eq!(cache.entries.get(&missing_child), Some(&false));
    }
}
