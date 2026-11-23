use anyhow::{Result, anyhow};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use crate::common::TildePath;
use crate::game::utils::path::{
    is_valid_wine_prefix, is_wine_prefix_path, path_selection_to_tilde, prompt_for_save_path,
};
use crate::menu::protocol;
use crate::menu_utils::{
    DialogOutcome, FilePickerScope, FzfSelectable, FzfWrapper, HeaderBuilder, PathInputBuilder,
    PathInputSelection,
};
use crate::restic::wrapper::Snapshot;
use crate::ui::catppuccin::{colors, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

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

pub(super) fn prompt_manual_save_path(
    game_name: &str,
    original_save_path: Option<&str>,
    enable_wine_prefix: bool,
) -> Result<Option<SelectedSavePath>> {
    let tilde_path = prompt_for_save_path(game_name, None, || {
        let prompt = format!(
            "{} Enter the save path for '{}' (e.g., ~/.local/share/{}/saves):",
            char::from(NerdFont::Edit),
            game_name,
            game_name.to_lowercase().replace(' ', "-")
        );

        let mut path_builder = PathInputBuilder::new()
            .header(HeaderBuilder::new(
                NerdFont::Folder,
                format!("Choose the save path for '{game_name}'"),
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
            ));

        if enable_wine_prefix {
            path_builder = path_builder.wine_prefix_option_label(format!(
                "{} Select a Wine prefix",
                char::from(NerdFont::Wine)
            ));
        }

        let path_selection = path_builder.choose()?;

        match path_selection {
            PathInputSelection::Manual(input) => {
                if input.trim().is_empty() {
                    FzfWrapper::message("Save path cannot be empty.")?;
                    Ok(None)
                } else {
                    path_selection_to_tilde(PathInputSelection::Manual(input))
                }
            }
            PathInputSelection::Picker(path) => {
                let final_path = if let Some(original) = original_save_path {
                    handle_differently_named_folders(&path, original)?.unwrap_or(path)
                } else {
                    path
                };
                Ok(Some(TildePath::new(final_path)))
            }
            PathInputSelection::WinePrefix(prefix_path) => {
                if !is_valid_wine_prefix(&prefix_path) {
                    FzfWrapper::message(
                        "Selected path is not a valid Wine prefix (missing drive_c directory).",
                    )?;
                    Ok(None)
                } else {
                    Ok(Some(TildePath::new(prefix_path)))
                }
            }
            PathInputSelection::Cancelled => Ok(None),
        }
    })?;

    Ok(tilde_path.map(|tilde| SelectedSavePath {
        display_path: tilde.display_string(),
        snapshot_path: None,
    }))
}

pub(super) fn choose_installation_path(
    game_name: &str,
    paths: &[PathInfo],
    original_save_path: Option<&str>,
) -> Result<Option<SelectedSavePath>> {
    let mut options = vec![SavePathOption::custom()];

    for path_info in paths {
        options.push(SavePathOption::snapshot(path_info.clone()));
    }

    let selected = FzfWrapper::builder()
        .prompt("Select")
        .header(
            HeaderBuilder::new(
                NerdFont::Folder,
                format!("Choose the save path for '{game_name}'"),
            )
            .subtitle("Pick a path from the backups or enter your own.")
            .build(),
        )
        .responsive_layout()
        .items(options)
        .padded()
        .select_one()
        .map_err(|e| anyhow!("Failed to select path option: {e}"))?;

    match selected {
        DialogOutcome::Submitted(option) => match option.kind {
            SavePathOptionKind::Custom => {
                // Enable wine prefix support if any snapshot paths are from wine prefixes
                let enable_wine_prefix = paths.iter().any(|path_info| {
                    path_info
                        .preferred_snapshot_path()
                        .map(is_wine_prefix_path)
                        .unwrap_or(false)
                });

                // Use the most common snapshot path as reference for directory name handling
                let reference_path = paths
                    .first()
                    .and_then(|p| p.preferred_snapshot_path())
                    .or(original_save_path);

                prompt_manual_save_path(game_name, reference_path, enable_wine_prefix)
            }
            SavePathOptionKind::Snapshot(path_info) => {
                Ok(Some(SelectedSavePath::from_path_info(&path_info)))
            }
        },
        DialogOutcome::Cancelled => {
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
        format!(
            "{} {}  {}",
            format_icon_colored(NerdFont::Folder, colors::LAVENDER),
            self.display_path,
            format_with_color(&format!("({})", self.usage()), colors::SUBTEXT0)
        )
    }

    fn fzf_key(&self) -> String {
        self.display_path.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        protocol::FzfPreview::Text(self.build_preview())
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

    /// How many backups used this path, and on which devices.
    fn usage(&self) -> String {
        let devices = if self.devices.len() == 1 {
            let device = self
                .devices
                .iter()
                .next()
                .map(String::as_str)
                .unwrap_or_default();
            format!("1 device: {device}")
        } else {
            format!(
                "{} devices: {}",
                self.devices.len(),
                self.devices.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        };
        format!(
            "used {} {times} on {devices}",
            self.frequency,
            times = if self.frequency == 1 { "time" } else { "times" }
        )
    }

    /// Plain path and usage, for previews that suggest this path.
    pub(super) fn summary(&self) -> String {
        format!("{} ({})", self.display_path, self.usage())
    }

    fn build_preview(&self) -> String {
        let device_count = match self.devices.len() {
            1 => "1 unique device".to_string(),
            count => format!("{count} unique devices"),
        };
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Folder, "Save Path")
            .field("Path", &self.display_path)
            .field("Snapshots", &format!("{} snapshots", self.frequency))
            .field("Devices", &device_count);

        if let Some(original) = self.preferred_snapshot_path() {
            builder = builder.field("Stored as", original);
        }

        builder = builder
            .blank()
            .separator()
            .line(
                colors::MAUVE,
                Some(NerdFont::Desktop),
                "Devices Using This Path",
            )
            .blank();
        for device in &self.devices {
            builder = builder.bullet(device);
        }

        if let (Ok(first), Ok(last)) = (
            chrono::DateTime::parse_from_rfc3339(&self.first_seen),
            chrono::DateTime::parse_from_rfc3339(&self.last_seen),
        ) {
            builder = builder
                .blank()
                .separator()
                .line(colors::MAUVE, Some(NerdFont::Calendar), "Usage Timeline")
                .blank()
                .field_indented("First seen", &first.format("%Y-%m-%d %H:%M:%S").to_string())
                .field_indented("Last seen", &last.format("%Y-%m-%d %H:%M:%S").to_string());
        }

        builder.build_string()
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
            SavePathOptionKind::Custom => format!(
                "{} Enter a different path",
                format_icon_colored(NerdFont::Edit, colors::BLUE)
            ),
            SavePathOptionKind::Snapshot(info) => info.fzf_display_text(),
        }
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        match &self.kind {
            SavePathOptionKind::Custom => PreviewBuilder::new()
                .header(NerdFont::Edit, "Enter a Different Path")
                .text("Provide a custom path for the game's save data.")
                .blank()
                .text("Browse for the save data or type its exact path.")
                .build(),
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

/// Checks if the selected path has a different folder name than the original save folder
/// and offers the user a choice between using the original folder name appended to the chosen path
/// or using the chosen path as is
fn handle_differently_named_folders(
    selected_path: &Path,
    original_save_path: &str,
) -> Result<Option<std::path::PathBuf>> {
    // Only handle this for directory saves, not files
    if selected_path.is_file() {
        return Ok(None);
    }

    // Extract the folder/file name from the original save path
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

        let header = HeaderBuilder::new(NerdFont::Info, "Directory name mismatch detected")
            .field(
                "Selected",
                format!(
                    "{} (ends with '{}')",
                    selected_path.display(),
                    selected_folder_name
                ),
            )
            .field(
                "Snapshot",
                format!("(ends with '{}')", original_folder_name),
            )
            .build();

        let options = vec![
            FolderNameOption::keep_selected(selected_path),
            FolderNameOption::append_original(&alternative_path),
        ];

        match FzfWrapper::builder()
            .header(header)
            .responsive_layout()
            .items(options)
            .padded()
            .select_one()?
        {
            DialogOutcome::Submitted(option) => Ok(Some(option.path)),
            DialogOutcome::Cancelled => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// What to do when the picked folder is named differently than the backed-up one.
#[derive(Clone, Copy)]
enum FolderNameChoice {
    /// Use the picked folder as the save path.
    KeepSelected,
    /// Use a folder with the backed-up name inside the picked one.
    AppendOriginal,
}

#[derive(Clone)]
struct FolderNameOption {
    choice: FolderNameChoice,
    /// The save path this option leads to.
    path: std::path::PathBuf,
}

impl FolderNameOption {
    fn keep_selected(selected_path: &Path) -> Self {
        Self {
            choice: FolderNameChoice::KeepSelected,
            path: selected_path.to_path_buf(),
        }
    }

    fn append_original(alternative_path: &Path) -> Self {
        Self {
            choice: FolderNameChoice::AppendOriginal,
            path: alternative_path.to_path_buf(),
        }
    }
}

impl FzfSelectable for FolderNameOption {
    fn fzf_display_text(&self) -> String {
        match self.choice {
            FolderNameChoice::KeepSelected => format!(
                "{} Use the selected path as is",
                format_icon_colored(NerdFont::Folder, colors::LAVENDER)
            ),
            FolderNameChoice::AppendOriginal => format!(
                "{} Append the original folder name",
                format_icon_colored(NerdFont::FolderOpen, colors::TEAL)
            ),
        }
    }

    fn fzf_key(&self) -> String {
        match self.choice {
            FolderNameChoice::KeepSelected => "keep-selected",
            FolderNameChoice::AppendOriginal => "append-original",
        }
        .to_string()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        let (icon, title, summary) = match self.choice {
            FolderNameChoice::KeepSelected => (
                NerdFont::Folder,
                "Use Selected Path",
                "Keep the folder you picked as the save path.",
            ),
            FolderNameChoice::AppendOriginal => (
                NerdFont::FolderOpen,
                "Append Original Folder",
                "Use a folder with the backed-up name inside the one you picked, matching the snapshot layout.",
            ),
        };

        protocol::FzfPreview::Text(
            PreviewBuilder::new()
                .header(icon, title)
                .text(summary)
                .blank()
                .field("Result", &self.path.display().to_string())
                .build_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};
    use crate::ui::text::strip_ansi;

    fn path_info(display_path: &str, frequency: usize, devices: &[&str]) -> PathInfo {
        PathInfo {
            display_path: display_path.to_string(),
            snapshot_paths: BTreeSet::new(),
            frequency,
            devices: devices.iter().map(|device| device.to_string()).collect(),
            first_seen: "2024-01-01T10:00:00Z".to_string(),
            last_seen: "2024-06-01T10:00:00Z".to_string(),
        }
    }

    fn plain_preview(item: &impl FzfSelectable) -> String {
        let protocol::FzfPreview::Text(text) = item.fzf_preview() else {
            panic!("expected a text preview");
        };
        strip_ansi(&text)
    }

    #[test]
    fn save_path_rows_pair_a_badge_with_the_usage() {
        let info = path_info("~/.config/celeste", 1, &["desktop"]);
        let row = info.fzf_display_text();
        assert!(row.contains(&format_icon_colored(NerdFont::Folder, colors::LAVENDER)));
        assert!(
            strip_ansi(&row).ends_with("~/.config/celeste  (used 1 time on 1 device: desktop)"),
            "{row:?}"
        );
        assert_eq!(
            info.summary(),
            "~/.config/celeste (used 1 time on 1 device: desktop)"
        );

        assert_eq!(
            path_info("~/saves", 3, &["desktop"]).usage(),
            "used 3 times on 1 device: desktop"
        );
    }

    #[test]
    fn save_path_previews_are_styled() {
        let text = plain_preview(&path_info("~/.config/celeste", 2, &["desktop"]));
        assert!(text.contains('─'), "missing the styled header: {text}");
        for expected in [
            "Save Path",
            "Path:",
            "~/.config/celeste",
            "Snapshots: 2 snapshots",
            "Devices: 1 unique device",
            "Devices Using This Path",
            "• desktop",
            "Usage Timeline",
            "First seen:",
            "2024-01-01 10:00:00",
            "Last seen:",
            "2024-06-01 10:00:00",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from:\n{text}"
            );
        }
        assert!(!text.contains("SAVE PATH DETAILS"));
    }

    #[test]
    fn custom_path_row_matches_the_manual_entry_style() {
        let custom = SavePathOption::custom();
        assert!(
            custom
                .fzf_display_text()
                .contains(&format_icon_colored(NerdFont::Edit, colors::BLUE))
        );
        assert!(plain_preview(&custom).contains("Enter a Different Path"));

        let snapshot = SavePathOption::snapshot(path_info("~/saves", 1, &["desktop"]));
        assert_eq!(snapshot.fzf_key(), "~/saves");
        assert!(plain_preview(&snapshot).contains("Devices Using This Path"));
    }

    #[test]
    fn folder_name_mismatch_returns_the_chosen_path_or_nothing() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let picked = temp.path().join("picked");
        // A matching folder name needs no choice at all.
        assert_eq!(
            handle_differently_named_folders(&picked, "/tmp/picked")?,
            None
        );

        let _mock = MockQueue::new()
            .select_index(0)
            .select_index(1)
            .cancel_selection()
            .guard();
        assert_eq!(
            handle_differently_named_folders(&picked, "/tmp/SaveData")?,
            Some(picked.clone())
        );
        assert_eq!(
            handle_differently_named_folders(&picked, "/tmp/SaveData")?,
            Some(picked.join("SaveData"))
        );
        assert_eq!(
            handle_differently_named_folders(&picked, "/tmp/SaveData")?,
            None
        );
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

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
