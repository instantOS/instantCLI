use anyhow::{Result, anyhow};
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::dot::path_serde::TildePath;
use crate::menu::protocol;
use crate::menu_utils::{
    FilePickerScope, FzfSelectable, FzfWrapper, PathInputBuilder, PathInputSelection,
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

pub(super) fn prompt_manual_save_path(game_name: &str) -> Result<Option<SelectedSavePath>> {
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
            let tilde = TildePath::new(path);
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
) -> Result<Option<SelectedSavePath>> {
    println!(
        "\n{} Select the save path for '{game_name}':",
        char::from(NerdFont::Folder)
    );

    let mut options = vec![SavePathOption::custom()];

    for path_info in paths {
        options.push(SavePathOption::snapshot(path_info.clone()));
    }

    let selected = FzfWrapper::select_one(options)
        .map_err(|e| anyhow!("Failed to select path option: {e}"))?;

    match selected {
        Some(option) => match option.kind {
            SavePathOptionKind::Custom => prompt_manual_save_path(game_name),
            SavePathOptionKind::Snapshot(path_info) => {
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
        self
            .snapshot_paths
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
