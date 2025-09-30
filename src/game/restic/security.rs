use crate::fzf_wrapper::{ConfirmResult, FzfWrapper};
use crate::game::config::{GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::game::games::selection;
use crate::game::utils::save_files::{
    TimeComparison, compare_snapshot_vs_local, format_file_size, format_system_time_for_display,
    get_save_directory_info,
};
use crate::game::utils::validation;
use crate::restic::wrapper::Snapshot;
use crate::ui::prelude::*;
use anyhow::{Context, Result};

/// Result of game selection process
#[derive(Clone)]
pub struct GameSelectionResult {
    pub game_name: String,
    pub installation: GameInstallation,
}

/// Result of security validation
#[derive(Clone)]
pub struct SecurityValidationResult {
    pub save_path_exists: bool,
    pub has_local_saves: bool,
    pub save_info: Option<crate::game::utils::save_files::SaveDirectoryInfo>,
}

/// Select a game and get its installation with proper error handling
pub fn get_game_installation(game_name: Option<String>) -> Result<GameSelectionResult> {
    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Check restic availability and game manager initialization
    validation::check_restic_and_game_manager(&game_config)?;

    // Get game name
    let game_name = match game_name {
        Some(name) => name,
        None => match selection::select_game_interactive(None)? {
            Some(name) => name,
            None => {
                return Ok(GameSelectionResult {
                    game_name: String::new(),
                    installation: GameInstallation {
                        game_name: crate::game::config::GameName(String::new()),
                        save_path: crate::dot::path_serde::TildePath::new(std::path::PathBuf::new()),
                        nearest_checkpoint: None,
                    },
                });
            }
        },
    };

    // Find the game installation
    let installation = match installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name)
    {
        Some(installation) => installation.clone(),
        None => {
            emit(
                Level::Error,
                "game.security.installation_missing",
                &format!(
                    "{} Error: No installation found for game '{game_name}'.",
                    char::from(Fa::TimesCircle)
                ),
                None,
            );
            emit(
                Level::Info,
                "game.security.hint.add",
                &format!(
                    "Please add the game first using '{} game add'.",
                    env!("CARGO_BIN_NAME")
                ),
                None,
            );
            return Err(anyhow::anyhow!("game installation not found"));
        }
    };

    Ok(GameSelectionResult {
        game_name,
        installation,
    })
}

/// Validate the restore environment and check security constraints
pub fn validate_restore_environment(
    installation: &GameInstallation,
) -> Result<SecurityValidationResult> {
    let save_path = installation.save_path.as_path();

    // Get save directory information
    let save_info =
        get_save_directory_info(save_path).context("Failed to analyze save directory")?;

    let has_local_saves = save_info.file_count > 0;

    // Check if save path exists (not an error for restore - will be created)
    let save_path_exists = save_path.exists();

    Ok(SecurityValidationResult {
        save_path_exists,
        has_local_saves,
        save_info: Some(save_info),
    })
}

/// Check snapshot vs local saves and generate security warning if needed
pub fn check_snapshot_vs_local_saves(
    snapshot: &Snapshot,
    save_info: &crate::game::utils::save_files::SaveDirectoryInfo,
    game_name: &str,
    force: bool,
) -> Result<bool> {
    if force {
        return Ok(true);
    }

    // If no local saves, no warning needed
    if save_info.file_count == 0 {
        return Ok(true); // Safe to proceed
    }

    // If we have local saves but no modification time, warn but allow
    let local_modified_time = match save_info.last_modified {
        Some(time) => time,
        None => {
            // Show warning but allow proceed
            let confirmed = FzfWrapper::builder()
                .message(format!(
                    "  Security Warning for '{game_name}':\n\nLocal save files exist but modification time cannot be determined.\n\nRestoring from snapshot might overwrite existing saves.\n\nDo you want to continue?"
                ))
                .yes_text("Continue Restore")
                .no_text("Cancel")
                .show_confirmation()
                .context("Failed to show security warning dialog")?;

            return Ok(confirmed == ConfirmResult::Yes);
        }
    };

    // Compare snapshot time with local save time
    let time_comparison = compare_snapshot_vs_local(&snapshot.time, local_modified_time)
        .context("Failed to compare snapshot and local save times")?;

    match time_comparison {
        TimeComparison::LocalNewer => {
            // Local saves are newer - this is potentially dangerous
            let local_time_str = format_system_time_for_display(Some(local_modified_time));
            let snapshot_time_str = format_snapshot_time_for_display(&snapshot.time);

            let confirmed = FzfWrapper::builder()
                .message(format!(
                    "  CRITICAL Security Warning for '{game_name}':\n\nLocal saves are NEWER than the selected snapshot!\n\n Local saves modified: {local_time_str}\n Snapshot created:   {snapshot_time_str}\n\nRestoring will OVERWRITE newer local saves with older data.\n\nThis action cannot be undone. Are you absolutely sure?"
                ))
                .yes_text("Overwrite Local Saves")
                .no_text("Cancel Restore")
                .show_confirmation()
                .context("Failed to show critical security warning dialog")?;

            Ok(confirmed == ConfirmResult::Yes)
        }
        TimeComparison::SnapshotNewer => {
            // Snapshot is newer - this is safe, but still inform user
            let local_time_str = format_system_time_for_display(Some(local_modified_time));
            let snapshot_time_str = format_snapshot_time_for_display(&snapshot.time);

            let confirmed = FzfWrapper::builder()
                .message(format!(
                    " Restore Summary for '{}':\n\nSnapshot is newer than local saves.\n\n Local saves modified: {}\n Snapshot created:   {}\n Local save files:   {} ({})\n\nDo you want to continue with the restore?",
                    game_name, local_time_str, snapshot_time_str, save_info.file_count, format_file_size(save_info.total_size)
                ))
                .yes_text("Continue Restore")
                .no_text("Cancel")
                .show_confirmation()
                .context("Failed to show restore summary dialog")?;

            Ok(confirmed == ConfirmResult::Yes)
        }
        TimeComparison::Same => {
            // Times are the same - this is safe
            Ok(true)
        }
        TimeComparison::Error(msg) => {
            // Error in comparison - warn but allow
            let confirmed = FzfWrapper::builder()
                .message(format!(
                    "  Warning for '{game_name}':\n\nCould not compare snapshot and local save times: {msg}\n\nRestoring might overwrite existing saves.\n\nDo you want to continue?"
                ))
                .yes_text("Continue Restore")
                .no_text("Cancel")
                .show_confirmation()
                .context("Failed to show comparison error warning")?;

            Ok(confirmed == ConfirmResult::Yes)
        }
    }
}

/// Format snapshot time for display (ISO to readable format)
fn format_snapshot_time_for_display(iso_time: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso_time) {
        Ok(parsed) => parsed.format("%Y-%m-%d %H:%M:%S").to_string(),
        Err(_) => iso_time.to_string(),
    }
}

/// Create enhanced restore confirmation dialog with security information
pub fn create_restore_confirmation(
    game_name: &str,
    snapshot: &Snapshot,
    security_result: &SecurityValidationResult,
    force: bool,
) -> Result<bool> {
    if force {
        return Ok(true);
    }

    let save_path = &security_result.save_info.as_ref().unwrap().last_modified;

    // Build confirmation message based on security context
    let mut message = format!(
        "{} Restore game saves for '{}' from snapshot {}\n\n",
        char::from(Fa::Download),
        game_name,
        &snapshot.id[..8.min(snapshot.id.len())]
    );

    if security_result.has_local_saves {
        message.push_str(" Current local saves:\n");
        if let Some(_modified_time) = save_path {
            message.push_str(&format!(
                "  • Last modified: {}\n",
                format_system_time_for_display(*save_path)
            ));
        }
        if let Some(info) = &security_result.save_info {
            message.push_str(&format!(
                "  • Files: {} ({})\n",
                info.file_count,
                format_file_size(info.total_size)
            ));
        }
        message.push('\n');
    } else {
        message.push_str(" No local save files found\n\n");
    }

    message.push_str(&format!(
        "  Snapshot from: {} on {}\n",
        format_snapshot_time_for_display(&snapshot.time),
        snapshot.hostname
    ));

    if let Some(summary) = &snapshot.summary {
        let total_files = summary.files_new + summary.files_changed + summary.files_unmodified;
        message.push_str(&format!(" Snapshot contains {total_files} files\n"));
    }

    message.push_str("\n  This will overwrite existing save files.\nThis action cannot be undone.\n\nAre you sure you want to continue?");

    let confirmed = FzfWrapper::builder()
        .message(&message)
        .yes_text("Restore")
        .no_text("Cancel")
        .show_confirmation()
        .context("Failed to show restore confirmation dialog")?;

    Ok(confirmed == ConfirmResult::Yes)
}

/// Validate snapshot ID exists for the given game
pub fn validate_snapshot_id(
    snapshot_id: &str,
    game_name: &str,
    config: &InstantGameConfig,
) -> Result<bool> {
    match super::cache::get_snapshot_by_id(snapshot_id, game_name, config)? {
        Some(_) => Ok(true),
        None => {
            emit(
                Level::Error,
                "game.security.snapshot_missing",
                &format!(
                    "{} Error: Snapshot '{snapshot_id}' not found for game '{game_name}'.",
                    char::from(Fa::TimesCircle)
                ),
                None,
            );
            emit(
                Level::Info,
                "game.security.snapshot_hint",
                "Please select a valid snapshot.",
                None,
            );
            Ok(false)
        }
    }
}
