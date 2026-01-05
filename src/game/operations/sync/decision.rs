use super::types::{SyncAction, ToleranceDirection};
use crate::game::config::{GameInstallation, InstantGameConfig};
use crate::game::restic::cache;
use crate::game::utils::save_files::{
    TimeComparison, compare_snapshot_vs_local, get_save_directory_info, SYNC_TOLERANCE_SECONDS,
};
use anyhow::Result;
use std::time::SystemTime;

/// Check if file was modified after checkpoint was set (with tolerance)
/// Returns true if file was modified significantly after checkpoint
fn file_modified_after_checkpoint(file_time: SystemTime, checkpoint_time: &str) -> bool {
    // Parse checkpoint time from ISO 8601 string
    let checkpoint_dt = match chrono::DateTime::parse_from_rfc3339(checkpoint_time) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => return true, // If we can't parse, assume modified (safer to backup)
    };

    // Convert file_time to DateTime
    let file_dt: chrono::DateTime<chrono::Utc> = file_time.into();

    // Calculate difference in seconds
    let diff_seconds = file_dt
        .signed_duration_since(checkpoint_dt)
        .num_seconds();

    // File is considered modified if it's more than SYNC_TOLERANCE_SECONDS newer than checkpoint
    diff_seconds > SYNC_TOLERANCE_SECONDS
}

/// Determine the required action for a single game
pub fn determine_action(
    installation: &GameInstallation,
    game_config: &InstantGameConfig,
    force: bool,
) -> Result<SyncAction> {
    let game_name = &installation.game_name.0;
    let save_path = installation.save_path.as_path();

    // Security check: ensure save directory exists
    // For single files, the file may not exist locally but could be restored from snapshots
    if !save_path.exists() {
        // For single file saves, check if we can restore from snapshots
        if installation.save_path_type.is_file() {
            let snapshots = cache::get_snapshots_for_game(game_name, game_config)?;
            if let Some(snapshot) = snapshots.first() {
                // Single file doesn't exist but snapshots exist - restore from latest
                // Note: We don't check checkpoint matching here since the file is missing locally
                return Ok(SyncAction::RestoreFromLatest(snapshot.id.clone()));
            } else {
                // No local file and no snapshots
                return Ok(SyncAction::Error(
                    "Save file does not exist and no snapshots found - nothing to sync".to_string(),
                ));
            }
        } else {
            // For directories, require existence
            return Ok(SyncAction::Error(format!(
                "Save path does not exist: {}",
                save_path.display()
            )));
        }
    }

    // Get local save information
    let local_save_info = get_save_directory_info(save_path)?;

    // Security check: ensure save directory is not empty before backing up
    // For single files that don't exist, we'll handle this in the snapshot comparison logic
    if local_save_info.file_count == 0 && save_path.exists() {
        return Ok(SyncAction::Error(
            "Save directory is empty - refusing to backup empty directory".to_string(),
        ));
    }

    // Get latest snapshot for this game
    let snapshots = cache::get_snapshots_for_game(game_name, game_config)?;
    let latest_snapshot = snapshots.first();

    // Determine sync action based on local saves and snapshots
    match (local_save_info.last_modified, latest_snapshot) {
        (Some(local_time), Some(snapshot)) => {
            // Both local saves and snapshots exist - compare timestamps
            match compare_snapshot_vs_local(&snapshot.time, local_time) {
                Ok(TimeComparison::LocalNewer) => {
                    // Check if backup should be skipped:
                    // 1. Checkpoint ID matches latest snapshot
                    // 2. File was NOT modified after checkpoint was set (within tolerance)
                    if !force
                        && let Some(ref nearest_checkpoint) = installation.nearest_checkpoint
                        && nearest_checkpoint == &snapshot.id
                        && let Some(ref checkpoint_time) = installation.checkpoint_time
                        && !file_modified_after_checkpoint(local_time, checkpoint_time)
                    {
                        return Ok(SyncAction::BackupSkipped(snapshot.id.clone()));
                    }
                    Ok(SyncAction::CreateBackup)
                }
                Ok(TimeComparison::LocalNewerWithinTolerance(delta)) => {
                    if force {
                        return Ok(SyncAction::CreateBackup);
                    }
                    Ok(SyncAction::WithinTolerance {
                        direction: ToleranceDirection::LocalNewer,
                        delta_seconds: delta,
                    })
                }
                Ok(TimeComparison::SnapshotNewer) => {
                    if !force
                        && let Some(ref nearest_checkpoint) = installation.nearest_checkpoint
                        && nearest_checkpoint == &snapshot.id
                    {
                        return Ok(SyncAction::RestoreSkipped(snapshot.id.clone()));
                    }
                    Ok(SyncAction::RestoreFromSnapshot(snapshot.id.clone()))
                }
                Ok(TimeComparison::SnapshotNewerWithinTolerance(delta)) => {
                    if force {
                        return Ok(SyncAction::RestoreFromSnapshot(snapshot.id.clone()));
                    }
                    if !force
                        && let Some(ref nearest_checkpoint) = installation.nearest_checkpoint
                        && nearest_checkpoint == &snapshot.id
                    {
                        return Ok(SyncAction::RestoreSkipped(snapshot.id.clone()));
                    }
                    Ok(SyncAction::WithinTolerance {
                        direction: ToleranceDirection::SnapshotNewer,
                        delta_seconds: delta,
                    })
                }
                Ok(TimeComparison::Same) => Ok(SyncAction::NoActionNeeded),
                Ok(TimeComparison::Error(e)) => {
                    Ok(SyncAction::Error(format!("Time comparison error: {e}")))
                }
                Err(e) => Ok(SyncAction::Error(format!(
                    "Failed to compare timestamps: {e}"
                ))),
            }
        }
        (Some(_local_time), None) => {
            // Local saves exist but no snapshots - create initial backup
            Ok(SyncAction::CreateInitialBackup)
        }
        (None, Some(snapshot)) => {
            // Check if restore should be skipped due to matching checkpoint
            if !force
                && let Some(ref nearest_checkpoint) = installation.nearest_checkpoint
                && nearest_checkpoint == &snapshot.id
            {
                return Ok(SyncAction::RestoreSkipped(snapshot.id.clone()));
            }

            // No local saves but snapshots exist - restore from latest
            Ok(SyncAction::RestoreFromLatest(snapshot.id.clone()))
        }
        (None, None) => {
            // No local saves and no snapshots
            Ok(SyncAction::Error(
                "No local saves and no snapshots found - nothing to sync".to_string(),
            ))
        }
    }
}
