use anyhow::{Context, Result};

use super::config::InstallationsConfig;
use super::restic::cache;

/// Helper function to update installation checkpoint
pub fn update_installation_checkpoint(game_name: &str, checkpoint_id: &str) -> Result<()> {
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Find and update the installation
    for installation in &mut installations.installations {
        if installation.game_name.0 == game_name {
            installation.update_checkpoint(checkpoint_id);
            break;
        }
    }

    installations
        .save()
        .context("Failed to save updated installations configuration")
}

/// Extract snapshot ID from backup result string
/// Handles both "snapshot: {id}" format and fallback to latest snapshot
pub fn extract_snapshot_id_from_backup_result(
    backup_result: &str,
    game_name: &str,
    game_config: &super::config::InstantGameConfig,
) -> Result<Option<String>> {
    if backup_result.starts_with("snapshot: ") {
        // Extract ID from "snapshot: {id}" format
        Ok(Some(
            backup_result
                .strip_prefix("snapshot: ")
                .unwrap_or(backup_result)
                .to_string(),
        ))
    } else {
        // Try to get the latest snapshot for this game as fallback
        match cache::get_snapshots_for_game(game_name, game_config) {
            Ok(snapshots) => {
                if let Some(latest) = snapshots.first() {
                    Ok(Some(latest.id.clone()))
                } else {
                    eprintln!("Warning: Could not determine snapshot ID for checkpoint update");
                    Ok(None)
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Could not fetch snapshots for checkpoint update: {}",
                    e
                );
                Ok(None)
            }
        }
    }
}

/// Update installation checkpoint after successful backup
pub fn update_checkpoint_after_backup(
    backup_result: &str,
    game_name: &str,
    game_config: &super::config::InstantGameConfig,
) -> Result<()> {
    if let Some(snapshot_id) =
        extract_snapshot_id_from_backup_result(backup_result, game_name, game_config)?
    {
        if let Err(e) = update_installation_checkpoint(game_name, &snapshot_id) {
            eprintln!("Warning: Could not update checkpoint: {}", e);
        }
    }
    Ok(())
}

/// Update installation checkpoint after successful restore
pub fn update_checkpoint_after_restore(game_name: &str, snapshot_id: &str) -> Result<()> {
    update_installation_checkpoint(game_name, snapshot_id)
}
