use anyhow::{Result, anyhow};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::dot::path_serde::TildePath;
use crate::menu::protocol;
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfSelectable, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::restic::wrapper::Snapshot;
use crate::ui::nerd_font::NerdFont;

pub(super) fn extract_unique_paths_from_snapshots(snapshots: &[Snapshot]) -> Result<Vec<PathInfo>> {
    let mut path_frequency: HashMap<String, PathInfo> = HashMap::new();

    for snapshot in snapshots {
        for path in &snapshot.paths {
            let normalized_path = normalize_path_for_cross_device(path);

            let entry = path_frequency
                .entry(normalized_path.clone())
                .or_insert_with(|| PathInfo {
                    display_path: normalized_path.clone(),
                    snapshot_paths: BTreeSet::new(),
                    frequency: 0,
                    devices: HashSet::new(),
                    first_seen: snapshot.time.clone(),
                    last_seen: snapshot.time.clone(),
                });

            entry.frequency += 1;
            entry.devices.insert(snapshot.hostname.clone());
            entry.snapshot_paths.insert(path.clone());

            if snapshot.time < entry.first_seen {
                entry.first_seen = snapshot.time.clone();
            }
            if snapshot.time > entry.last_seen {
                entry.last_seen = snapshot.time.clone();
            }
        }
    }

    let mut paths: Vec<PathInfo> = path_frequency.into_values().collect();

    paths.sort_by(|a, b| {
        b.frequency
            .cmp(&a.frequency)
            .then(b.devices.len().cmp(&a.devices.len()))
    });

    Ok(paths)
}

#[derive(Debug, Clone)]
pub(super) struct SelectedSavePath {
    pub display_path: String,
    pub snapshot_path: Option<String>,
}

impl SelectedSavePath {
    fn from_path_info(path_info: &PathInfo) -> Self {
        let snapshot_path = path_info
            .preferred_snapshot_path()
            .map(|path| path.to_string());

        Self {
            display_path: path_info.display_path.clone(),
            snapshot_path,
        }
    }
}

pub(super) fn prompt_manual_save_path(game_name: &str, original_save_path: Option<&str>) -> Result<Option<SelectedSavePath>> {
    let prompt = format!(
        "{} Enter the save path for '{}' (e.g., ~/.local/share/{}/saves):",
        char::from(NerdFont::Edit),
        game_name,
        game_name.to_lowercase().replace(' ', "-")
    );

    let path_selection = PathInputBuilder::new()
        .header(format!(
            "{} Choose the save path for '{game_name}'",
            char::from(NerdFont::Folder)
        ))
        .manual_prompt(prompt)
        .scope(FilePickerScope::FilesAndDirectories)
        .picker_hint(format!(
            "{} Select the file or directory to use for {game_name} save data",
            char::from(NerdFont::Info)
        ))
        .manual_option_label(format!("{} Type an exact path", char::from(NerdFont::Edit)))
        .picker_option_label(format!(
            "{} Browse and choose a path",
            char::from(NerdFont::FolderOpen)
        ))
        .choose()?;

    match path_selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                println!("Empty path provided. Setup cancelled.");
                Ok(None)
            } else {
                let tilde =
                    TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid save path: {e}"))?;
                let display_path = tilde_display_string(&tilde);
                Ok(Some(SelectedSavePath {
                    display_path,
                    snapshot_path: None,
                }))
            }
        }
        PathInputSelection::Picker(path) => {
            // Handle differently named folders if we have an original save path
            let final_path = if let Some(original) = original_save_path {
                if let Some(adjusted_path) = handle_differently_named_folders(&path, original)? {
                    adjusted_path
                } else {
                    path
                }
            } else {
                path
            };
            
            let tilde = TildePath::new(final_path);
            let display_path = tilde_display_string(&tilde);
            Ok(Some(SelectedSavePath {
                display_path,
                snapshot_path: None,
            }))
        }
        PathInputSelection::WinePrefix(prefix_path) => {
            // For wine prefix selection, we need to validate it and then construct the path
            if !is_valid_wine_prefix(&prefix_path) {
                println!(
                    "{} Selected path is not a valid Wine prefix (missing drive_c directory).",
                    char::from(NerdFont::Warning)
                );
                return Ok(None);
            }
            
            let tilde = TildePath::new(prefix_path);
            let display_path = tilde_display_string(&tilde);
            Ok(Some(SelectedSavePath {
                display_path,
                snapshot_path: None,
            }))
        }
        PathInputSelection::Cancelled => {
            println!(
                "{} No path selected. Setup cancelled.",
                char::from(NerdFont::Warning)
            );
            Ok(None)
        }
    }
}

pub(super) fn choose_installation_path(
    game_name: &str,
    paths: &[PathInfo],
    original_save_path: Option<&str>,
) -> Result<Option<SelectedSavePath>> {
    println!(
        "\n{} Select the save path for '{game_name}':",
        char::from(NerdFont::Folder)
    );

    let mut options = vec![SavePathOption::custom()];
    
    // Check if any of the paths are from wine prefixes
    let mut wine_prefix_paths = Vec::new();
    for path_info in paths {
        if let Some(snapshot_path) = path_info.preferred_snapshot_path() {
            if is_wine_prefix_path(snapshot_path) {
                if let Some(relative_path) = extract_wine_relative_path(snapshot_path) {
                    wine_prefix_paths.push((path_info.clone(), relative_path));
                }
            }
        }
    }

    for path_info in paths {
        options.push(SavePathOption::snapshot(path_info.clone()));
    }

    let selected = FzfWrapper::select_one(options)
        .map_err(|e| anyhow!("Failed to select path option: {e}"))?;

    match selected {
        Some(option) => match option.kind {
            SavePathOptionKind::Custom => prompt_manual_save_path(game_name, None),
            SavePathOptionKind::Snapshot(path_info) => {
                // Check if this is a wine prefix path and offer the wine prefix option
                if let Some(snapshot_path) = path_info.preferred_snapshot_path() {
                    if is_wine_prefix_path(snapshot_path) {
                        if let Some(relative_path) = extract_wine_relative_path(snapshot_path) {
                            // Offer wine prefix option
                            let prompt = format!(
                                "{} This path appears to be from a Wine prefix. Would you like to select a Wine prefix instead?",
                                char::from(NerdFont::Wine)
                            );
                            
                            let confirm = FzfWrapper::builder()
                                .confirm(&prompt)
                                .yes_text("Select Wine Prefix")
                                .no_text("Use Path As Is")
                                .confirm_dialog()?;
                            
                            match confirm {
                                ConfirmResult::Yes => {
                                    // Prompt for wine prefix selection
                                    let wine_prefix_selection = PathInputBuilder::new()
                                        .header(format!(
                                            "{} Select Wine prefix for '{game_name}'",
                                            char::from(NerdFont::Wine)
                                        ))
                                        .manual_prompt(format!(
                                            "{} Enter the Wine prefix path:",
                                            char::from(NerdFont::Edit)
                                        ))
                                        .scope(FilePickerScope::Directories)
                                        .picker_hint(format!(
                                            "{} Select the Wine prefix directory (should contain drive_c)",
                                            char::from(NerdFont::Info)
                                        ))
                                        .manual_option_label(format!("{} Type Wine prefix path", char::from(NerdFont::Edit)))
                                        .picker_option_label(format!(
                                            "{} Browse for Wine prefix",
                                            char::from(NerdFont::FolderOpen)
                                        ))
                                        .wine_prefix_option_label(format!(
                                            "{} Select Wine prefix",
                                            char::from(NerdFont::Wine)
                                        ))
                                        .choose()?;
                                        
                                    match wine_prefix_selection {
                                        PathInputSelection::Manual(input) => {
                                            let prefix_path = PathBuf::from(input);
                                            if !is_valid_wine_prefix(&prefix_path) {
                                                println!(
                                                    "{} Selected path is not a valid Wine prefix (missing drive_c directory).",
                                                    char::from(NerdFont::Warning)
                                                );
                                                return Ok(None);
                                            }
                                            
                                            let full_path = wine_prefix_path(&prefix_path, &relative_path);
                                            let tilde = TildePath::new(full_path);
                                            let display_path = tilde_display_string(&tilde);
                                            return Ok(Some(SelectedSavePath {
                                                display_path,
                                                snapshot_path: Some(snapshot_path.to_string()),
                                            }));
                                        }
                                        PathInputSelection::Picker(prefix_path) |
                                        PathInputSelection::WinePrefix(prefix_path) => {
                                            if !is_valid_wine_prefix(&prefix_path) {
                                                println!(
                                                    "{} Selected path is not a valid Wine prefix (missing drive_c directory).",
                                                    char::from(NerdFont::Warning)
                                                );
                                                return Ok(None);
                                            }
                                            
                                            let full_path = wine_prefix_path(&prefix_path, &relative_path);
                                            let tilde = TildePath::new(full_path);
                                            let display_path = tilde_display_string(&tilde);
                                            return Ok(Some(SelectedSavePath {
                                                display_path,
                                                snapshot_path: Some(snapshot_path.to_string()),
                                            }));
                                        }
                                        PathInputSelection::Cancelled => {
                                            println!(
                                                "{} No path selected. Setup cancelled.",
                                                char::from(NerdFont::Warning)
                                            );
                                            return Ok(None);
                                        }
                                    }
                                }
                                ConfirmResult::No | ConfirmResult::Cancelled => {
                                    // Continue with normal path selection
                                }
                            }
                        }
                    }
                }
                
                Ok(Some(SelectedSavePath::from_path_info(&path_info)))
            }
        },
        None => {
            println!(
                "{} No path selected. Setup cancelled.",
                char::from(NerdFont::Warning)
            );
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PathInfo {
    pub display_path: String,
    pub snapshot_paths: BTreeSet<String>,
    frequency: usize,
    devices: HashSet<String>,
    first_seen: String,
    last_seen: String,
}

impl FzfSelectable for PathInfo {
    fn fzf_display_text(&self) -> String {
        let devices_str = if self.devices.len() == 1 {
            format!("1 device: {}", self.devices.iter().next().unwrap())
        } else {
            format!(
                "{} devices: {}",
                self.devices.len(),
                self.devices.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        };

        format!(
            "{} (used {} times on {})",
            self.display_path, self.frequency, devices_str
        )
    }

    fn fzf_key(&self) -> String {
        self.display_path.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        let mut preview = String::new();
        preview.push_str(&format!(
            "{} SAVE PATH DETAILS\n\n",
            char::from(NerdFont::Folder)
        ));
        preview.push_str(&format!("Path:           {}\n", self.display_path));
        preview.push_str(&format!("Usage Count:    {} snapshots\n", self.frequency));
        preview.push_str(&format!(
            "Device Count:   {} unique devices\n",
            self.devices.len()
        ));

        if let Some(original) = self.preferred_snapshot_path() {
            preview.push_str(&format!("Stored As:      {}\n", original));
        }

        preview.push_str(&format!(
            "\n{}  DEVICES USING THIS PATH:\n",
            char::from(NerdFont::Desktop)
        ));
        for device in &self.devices {
            preview.push_str(&format!("  • {device}\n"));
        }

        if let (Ok(first), Ok(last)) = (
            chrono::DateTime::parse_from_rfc3339(&self.first_seen),
            chrono::DateTime::parse_from_rfc3339(&self.last_seen),
        ) {
            let first_str = first.format("%Y-%m-%d %H:%M:%S").to_string();
            let last_str = last.format("%Y-%m-%d %H:%M:%S").to_string();

            preview.push_str("\n📅 USAGE TIMELINE:\n");
            preview.push_str(&format!("First Seen:     {first_str}\n"));
            preview.push_str(&format!("Last Seen:      {last_str}\n"));
        }

        protocol::FzfPreview::Text(preview)
    }
}

impl PathInfo {
    pub fn preferred_snapshot_path(&self) -> Option<&str> {
        self.snapshot_paths
            .iter()
            .find(|path| path.starts_with('/'))
            .map(|p| p.as_str())
            .or_else(|| self.snapshot_paths.iter().next().map(|p| p.as_str()))
    }
}

fn normalize_path_for_cross_device(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("/home/") {
        if let Some(slash_pos) = rest.find('/') {
            let after_user = &rest[slash_pos..];
            return format!("~{after_user}");
        } else {
            return "~".to_string();
        }
    }

    path.to_string()
}

fn tilde_display_string(tilde: &TildePath) -> String {
    tilde
        .to_tilde_string()
        .unwrap_or_else(|_| tilde.as_path().to_string_lossy().to_string())
}

#[derive(Clone)]
struct SavePathOption {
    kind: SavePathOptionKind,
}

#[derive(Clone)]
enum SavePathOptionKind {
    Custom,
    Snapshot(PathInfo),
}

impl SavePathOption {
    fn custom() -> Self {
        Self {
            kind: SavePathOptionKind::Custom,
        }
    }

    fn snapshot(path: PathInfo) -> Self {
        Self {
            kind: SavePathOptionKind::Snapshot(path),
        }
    }
}

impl FzfSelectable for SavePathOption {
    fn fzf_display_text(&self) -> String {
        match &self.kind {
            SavePathOptionKind::Custom => {
                format!("{} Enter a different path", char::from(NerdFont::Edit))
            }
            SavePathOptionKind::Snapshot(info) => info.fzf_display_text(),
        }
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        match &self.kind {
            SavePathOptionKind::Custom => protocol::FzfPreview::Text(format!(
                "{} Provide a custom path for the game's save data.",
                char::from(NerdFont::Info)
            )),
            SavePathOptionKind::Snapshot(info) => info.fzf_preview(),
        }
    }

    fn fzf_key(&self) -> String {
        match &self.kind {
            SavePathOptionKind::Custom => "CUSTOM".to_string(),
            SavePathOptionKind::Snapshot(info) => info.fzf_key(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_for_cross_device() {
        assert_eq!(
            normalize_path_for_cross_device("/home/alice/.config/game/saves"),
            "~/.config/game/saves"
        );

        assert_eq!(
            normalize_path_for_cross_device("/home/bob/Documents/GameSaves"),
            "~/Documents/GameSaves"
        );

        assert_eq!(normalize_path_for_cross_device("/home/alice"), "~");

        assert_eq!(
            normalize_path_for_cross_device("/opt/game/saves"),
            "/opt/game/saves"
        );

        assert_eq!(
            normalize_path_for_cross_device("~/.local/share/game"),
            "~/.local/share/game"
        );

        assert_eq!(normalize_path_for_cross_device("/"), "/");

        assert_eq!(
            normalize_path_for_cross_device("/home/user.name/.local/share/Steam/steamapps"),
            "~/.local/share/Steam/steamapps"
        );
    }
}

/// Checks if the selected path has a different folder name than the original save folder
/// and offers the user a choice between using the original folder name appended to the chosen path
/// or using the chosen path as is
fn handle_differently_named_folders(
    selected_path: &Path,
    original_save_path: &str,
) -> Result<Option<std::path::PathBuf>> {
    // Extract the folder name from the original save path
    let original_folder_name = Path::new(original_save_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
        
    if original_folder_name.is_empty() {
        return Ok(None);
    }
    
    // Get the selected path's folder name
    let selected_folder_name = selected_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
        
    // If the folder names are different, offer the user a choice
    if selected_folder_name != original_folder_name {
        let alternative_path = selected_path.join(original_folder_name);
        
        let prompt = format!(
            "{} Chosen directory name ({}) is different than the original save folder name ({}). Do you want to use the original folder name appended to the chosen path, or use the chosen path as is?",
            char::from(NerdFont::Info),
            selected_folder_name,
            original_folder_name
        );
        
        let options = vec![
            format!("Use selected path as is: {}", selected_path.display()),
            format!("Use alternative path: {}", alternative_path.display()),
        ];
        
        match FzfWrapper::select_one(options)? {
            Some(selected) => {
                if selected.contains("Use selected path as is") {
                    Ok(Some(selected_path.to_path_buf()))
                } else {
                    Ok(Some(alternative_path))
                }
            }
            None => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// Extracts the relative path from a Wine prefix path
/// For example, given "/home/user/.wine/drive_c/users/user/AppData/Local/LOA/Saved",
/// this would return "users/user/AppData/Local/LOA/Saved"
fn extract_wine_relative_path(path: &str) -> Option<String> {
    if let Some(pos) = path.find("/drive_c/") {
        let after_drive_c = &path[pos + "/drive_c/".len()..];
        if !after_drive_c.is_empty() {
            return Some(after_drive_c.to_string());
        }
    }
    None
}

/// Validates that a path is a valid Wine prefix by checking for the presence of a drive_c directory
pub fn is_valid_wine_prefix(path: &Path) -> bool {
    let drive_c_path = path.join("drive_c");
    drive_c_path.exists() && drive_c_path.is_dir()
}

/// Converts a Wine prefix path and a relative path within the prefix to a full path
/// For example, given prefix "/home/user/.wine" and relative path "users/user/AppData/Local/LOA/Saved",
/// this would return "/home/user/.wine/drive_c/users/user/AppData/Local/LOA/Saved"
pub fn wine_prefix_path(prefix: &Path, relative_path: &str) -> std::path::PathBuf {
    prefix.join("drive_c").join(relative_path)
}

/// Checks if a path appears to be from a Wine prefix by looking for the AppData pattern
fn is_wine_prefix_path(path: &str) -> bool {
    path.contains("/AppData/") || (path.contains("/users/") && path.contains("/drive_c/"))
}
