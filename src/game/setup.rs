use crate::ui::prelude::*;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

use crate::dot::path_serde::TildePath;
use crate::fzf_wrapper::{ConfirmResult, FzfSelectable, FzfWrapper};
use crate::game::config::{GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::game::games::validation::validate_game_manager_initialized;
use crate::game::restic::backup::GameBackup;
use crate::game::restic::cache;
use crate::game::utils::save_files::get_save_directory_info;
use crate::menu::protocol;

/// Set up games that have been added but don't have installations configured on this device
pub fn setup_uninstalled_games() -> Result<()> {
    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Check if game manager is initialized
    if !validate_game_manager_initialized()? {
        return Ok(());
    }

    // Find games without installations
    let uninstalled_games = find_uninstalled_games(&game_config, &installations)?;

    if uninstalled_games.is_empty() {
        success(
            "game.setup.all_configured",
            &format!(
                "{} All games are already configured for this device!",
                Icons::CHECK
            ),
        );
        return Ok(());
    }

    println!(
        "Found {} game(s) that need to be set up on this device:\n",
        uninstalled_games.len()
    );

    for game_name in &uninstalled_games {
        println!("  • {game_name}");
    }

    println!();

    // Process each uninstalled game
    for game_name in uninstalled_games {
        if let Err(e) = setup_single_game(&game_name, &game_config, &mut installations) {
            error(
                "game.setup.failed",
                &format!("{} Failed to set up game '{game_name}': {e}", Icons::ERROR),
            );

            // Ask if user wants to continue with other games
            match FzfWrapper::confirm("Would you like to continue setting up the remaining games?")
                .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => continue,
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Setup cancelled by user.");
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Find games that are in the config but don't have installations
fn find_uninstalled_games(
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
) -> Result<Vec<String>> {
    let installed_games: HashSet<_> = installations
        .installations
        .iter()
        .map(|inst| &inst.game_name.0)
        .collect();

    let uninstalled_games: Vec<String> = game_config
        .games
        .iter()
        .filter(|game| !installed_games.contains(&game.name.0))
        .map(|game| game.name.0.clone())
        .collect();

    Ok(uninstalled_games)
}

/// Set up a single game by collecting paths from snapshots and letting user choose
fn setup_single_game(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
) -> Result<()> {
    info(
        "game.setup.start",
        &format!("{} Setting up game: {game_name}", Icons::INFO),
    );

    // Get all snapshots for this game to extract paths
    let snapshots = cache::get_snapshots_for_game(game_name, game_config)
        .context("Failed to get snapshots for game")?;
    let latest_snapshot_id = snapshots.first().map(|snapshot| snapshot.id.clone());

    if snapshots.is_empty() {
        warn(
            "game.setup.no_snapshots",
            &format!("{} No snapshots found for game '{game_name}'.", Icons::WARN),
        );
        info(
            "game.setup.hint.add",
            &format!(
                "This game has no backups yet. You'll need to add an installation manually using '{} game add'.",
                env!("CARGO_BIN_NAME")
            ),
        );
        return Ok(());
    }

    // Extract unique paths from all snapshots
    let unique_paths = extract_unique_paths_from_snapshots(&snapshots)?;

    if unique_paths.is_empty() {
        warn(
            "game.setup.no_paths",
            &format!(
                "{} No save paths found in snapshots for game '{game_name}'.",
                Icons::WARN
            ),
        );
        info(
            "game.setup.hint.manual",
            "This is unusual. You may need to set up the installation manually.",
        );
        return Ok(());
    }

    println!(
        "\nFound {} unique save path(s) from different devices/snapshots:",
        unique_paths.len()
    );

    // Let user choose from available paths or enter a custom one
    let chosen_path = choose_installation_path(game_name, &unique_paths)?;

    if let Some(path_str) = chosen_path {
        // Convert to TildePath
        let save_path = TildePath::from_str(&path_str)
            .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?;
        let mut installation = GameInstallation::new(game_name, save_path.clone());

        let mut directory_created = false;
        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "Save path '{path_str}' does not exist. Would you like to create it?"
            ))
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => {
                    std::fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    success(
                        "game.setup.dir_created",
                        &format!("{} Created save directory: {path_str}", Icons::CHECK),
                    );
                    directory_created = true;
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Directory not created. You can create it later when needed.");
                }
            }
        }

        let save_dir_info = get_save_directory_info(save_path.as_path())
            .with_context(|| format!("Failed to inspect save directory '{path_str}'"))?;
        let path_exists_after = save_path.as_path().exists();
        let should_restore =
            path_exists_after && (directory_created || save_dir_info.file_count == 0);

        if should_restore {
            if let Some(snapshot_id) = latest_snapshot_id.as_deref() {
                info(
                    "game.setup.restore_latest",
                    &format!(
                        "{} Restoring latest backup ({snapshot_id}) into {path_str}...",
                        Icons::DOWNLOAD
                    ),
                );
                let restore_summary =
                    restore_latest_backup(game_name, &save_path, snapshot_id, game_config)?;
                success("game.setup.restore_done", &restore_summary);
                installation.update_checkpoint(snapshot_id.to_string());
            }
        }
        installations.installations.push(installation);
        installations.save()?;

        success(
            "game.setup.success",
            &format!(
                "{} Game '{game_name}' set up successfully with save path: {path_str}",
                Icons::CHECK
            ),
        );
    } else {
        warn(
            "game.setup.cancelled",
            &format!("Setup cancelled for game '{game_name}'."),
        );
    }

    println!();
    Ok(())
}

fn restore_latest_backup(
    game_name: &str,
    save_path: &TildePath,
    snapshot_id: &str,
    game_config: &InstantGameConfig,
) -> Result<String> {
    let backup_handler = GameBackup::new(game_config.clone());
    let summary = backup_handler
        .restore_game_backup(game_name, snapshot_id, save_path.as_path())
        .context("Failed to restore latest backup")?;

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(game_name, &repo_path);

    Ok(summary)
}

/// Extract unique paths from all snapshots, grouped by frequency for better presentation
fn extract_unique_paths_from_snapshots(
    snapshots: &[crate::restic::wrapper::Snapshot],
) -> Result<Vec<PathInfo>> {
    let mut path_frequency: HashMap<String, PathInfo> = HashMap::new();

    for snapshot in snapshots {
        for path in &snapshot.paths {
            // Normalize the path to use tilde notation for cross-device compatibility
            let normalized_path = normalize_path_for_cross_device(path);

            let entry = path_frequency
                .entry(normalized_path.clone())
                .or_insert(PathInfo {
                    path: normalized_path,
                    frequency: 0,
                    devices: HashSet::new(),
                    first_seen: snapshot.time.clone(),
                    last_seen: snapshot.time.clone(),
                });

            entry.frequency += 1;
            entry.devices.insert(snapshot.hostname.clone());

            // Update first/last seen (simple string comparison should work for ISO dates)
            if snapshot.time < entry.first_seen {
                entry.first_seen = snapshot.time.clone();
            }
            if snapshot.time > entry.last_seen {
                entry.last_seen = snapshot.time.clone();
            }
        }
    }

    let mut paths: Vec<PathInfo> = path_frequency.into_values().collect();

    // Sort by frequency (most common first) and then by number of devices
    paths.sort_by(|a, b| {
        b.frequency
            .cmp(&a.frequency)
            .then(b.devices.len().cmp(&a.devices.len()))
    });

    Ok(paths)
}

/// Normalize a path for cross-device compatibility by converting /home/<user> to ~
fn normalize_path_for_cross_device(path: &str) -> String {
    // Check if the path starts with /home/
    if let Some(rest) = path.strip_prefix("/home/") {
        // Find the first slash after the username
        if let Some(slash_pos) = rest.find('/') {
            // Extract the part after /home/<username>
            let after_user = &rest[slash_pos..];
            return format!("~{after_user}");
        } else {
            // The path is just /home/<username>, convert to ~
            return "~".to_string();
        }
    }

    // If the path doesn't start with /home/, return it as-is
    path.to_string()
}

/// Information about a path found in snapshots
#[derive(Debug, Clone)]
struct PathInfo {
    path: String,
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
            self.path, self.frequency, devices_str
        )
    }

    fn fzf_key(&self) -> String {
        self.path.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        let mut preview = String::new();
        preview.push_str("📁 SAVE PATH DETAILS\n\n");
        preview.push_str(&format!("Path:           {}\n", self.path));
        preview.push_str(&format!("Usage Count:    {} snapshots\n", self.frequency));
        preview.push_str(&format!(
            "Device Count:   {} unique devices\n",
            self.devices.len()
        ));

        preview.push_str("\n🖥️  DEVICES USING THIS PATH:\n");
        for device in &self.devices {
            preview.push_str(&format!("  • {device}\n"));
        }

        // Format dates
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

/// Simple wrapper for string options in fzf
#[derive(Debug, Clone)]
struct StringOption {
    text: String,
    value: String,
}

impl StringOption {
    fn new(text: String, value: String) -> Self {
        Self { text, value }
    }
}

impl FzfSelectable for StringOption {
    fn fzf_display_text(&self) -> String {
        self.text.clone()
    }

    fn fzf_key(&self) -> String {
        self.value.clone()
    }
}

/// Let user choose from available paths or enter a custom one
fn choose_installation_path(game_name: &str, paths: &[PathInfo]) -> Result<Option<String>> {
    //TODO: this should be an fzf wrapper message, as it is followed by a choice
    println!("\nChoose how to set up the save path for '{game_name}':");

    // Create options including the paths and a custom option
    let mut options = vec![StringOption::new(
        "[Enter custom path]".to_string(),
        "CUSTOM".to_string(),
    )];

    // Add existing paths
    for path_info in paths {
        options.push(StringOption::new(
            path_info.fzf_display_text(),
            path_info.path.clone(),
        ));
    }

    let selected = FzfWrapper::select_one(options)
        .map_err(|e| anyhow::anyhow!("Failed to select path option: {}", e))?;

    match selected {
        Some(selection) => {
            if selection.value == "CUSTOM" {
                // User wants to enter a custom path
                let custom_path = FzfWrapper::input(&format!(
                    "Enter custom save path for '{}' (e.g., ~/.local/share/{}/saves):",
                    game_name,
                    game_name.to_lowercase().replace(' ', "-")
                ))
                .map_err(|e| anyhow::anyhow!("Failed to get custom path input: {}", e))?
                .trim()
                .to_string();

                if custom_path.is_empty() {
                    println!("Empty path provided. Setup cancelled.");
                    return Ok(None);
                }

                Ok(Some(custom_path))
            } else {
                // User selected one of the existing paths
                Ok(Some(selection.value))
            }
        }
        None => {
            println!("No path selected. Setup cancelled.");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_for_cross_device() {
        // Test /home/<user>/<path> -> ~/<path>
        assert_eq!(
            normalize_path_for_cross_device("/home/alice/.config/game/saves"),
            "~/.config/game/saves"
        );

        assert_eq!(
            normalize_path_for_cross_device("/home/bob/Documents/GameSaves"),
            "~/Documents/GameSaves"
        );

        // Test /home/<user> -> ~
        assert_eq!(normalize_path_for_cross_device("/home/alice"), "~");

        // Test paths that don't start with /home/
        assert_eq!(
            normalize_path_for_cross_device("/opt/game/saves"),
            "/opt/game/saves"
        );

        assert_eq!(
            normalize_path_for_cross_device("~/.local/share/game"),
            "~/.local/share/game"
        );

        // Test root path
        assert_eq!(normalize_path_for_cross_device("/"), "/");

        // Test complex paths with usernames that might contain slashes conceptually
        assert_eq!(
            normalize_path_for_cross_device("/home/user.name/.local/share/Steam/steamapps"),
            "~/.local/share/Steam/steamapps"
        );
    }
}
