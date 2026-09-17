use super::types::{SyncAction, ToleranceDirection};
use crate::game::config::{GameInstallation, InstantGameConfig};
use crate::game::restic::cache;
use crate::game::utils::save_files::{
    SYNC_TOLERANCE_SECONDS, TimeComparison, compare_snapshot_vs_local, get_save_directory_info,
};
use crate::restic::wrapper::Snapshot;
use anyhow::{Result, bail};
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
    let diff_seconds = file_dt.signed_duration_since(checkpoint_dt).num_seconds();

    // File is considered modified if it's more than SYNC_TOLERANCE_SECONDS newer than checkpoint
    diff_seconds > SYNC_TOLERANCE_SECONDS
}

/// An explicit historical restore acknowledges the head without claiming its contents.
/// Only subsequent local edits should trigger a backup; even force must not undo
/// that choice by automatically restoring the same known head.
fn acknowledged_head_action(
    installation: &GameInstallation,
    snapshot: &Snapshot,
    local_time: Option<SystemTime>,
) -> Option<SyncAction> {
    if installation.nearest_checkpoint.is_none()
        || !installation
            .acknowledged_snapshot
            .as_deref()
            .is_some_and(|acknowledged| snapshot.matches_id(acknowledged))
    {
        return None;
    }

    if let (Some(local_time), Some(checkpoint_time)) =
        (local_time, installation.checkpoint_time.as_deref())
        && file_modified_after_checkpoint(local_time, checkpoint_time)
    {
        return Some(SyncAction::CreateBackup);
    }

    Some(SyncAction::RestoreSkipped(snapshot.id.clone()))
}

/// Determine the required action for a single game
pub fn determine_action(
    installation: &GameInstallation,
    game_config: &InstantGameConfig,
    force: bool,
) -> Result<SyncAction> {
    // These guards precede all filesystem inspection and snapshot access, even with force.
    if let Some(snapshot_id) = &installation.pending_restore {
        bail!(
            "Restore of snapshot '{snapshot_id}' is incomplete; run `ins game setup` to retry before syncing"
        );
    }
    if installation.needs_repository_reconciliation(&game_config.repo.as_path().to_string_lossy()) {
        bail!("Sync repository has changed; run `ins game setup` for repository reconciliation");
    }

    let game_name = &installation.game_name.0;
    let save_path = installation.save_path.as_path();

    // Security check: ensure save directory exists
    // For single files, the file may not exist locally but could be restored from snapshots
    if !save_path.exists() {
        // For single file saves, check if we can restore from snapshots
        if installation.save_path_type.is_file() {
            let snapshots = cache::get_snapshots_for_game(game_name, game_config)?;
            if let Some(snapshot) = snapshots.first() {
                if let Some(action) = acknowledged_head_action(installation, snapshot, None) {
                    return Ok(action);
                }
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

    if let Some(snapshot) = latest_snapshot
        && let Some(action) =
            acknowledged_head_action(installation, snapshot, local_save_info.last_modified)
    {
        return Ok(action);
    }

    // Determine sync action based on local saves and snapshots
    match (local_save_info.last_modified, latest_snapshot) {
        (Some(local_time), Some(snapshot)) => {
            // Both local saves and snapshots exist - compare timestamps
            match compare_snapshot_vs_local(&snapshot.time, local_time) {
                TimeComparison::LocalNewer => {
                    // Check if backup should be skipped:
                    // 1. Checkpoint ID matches latest snapshot
                    // 2. File was NOT modified after checkpoint was set (within tolerance)
                    if !force
                        && installation.checkpoint_matches(snapshot)
                        && let Some(ref checkpoint_time) = installation.checkpoint_time
                        && !file_modified_after_checkpoint(local_time, checkpoint_time)
                    {
                        return Ok(SyncAction::BackupSkipped(snapshot.id.clone()));
                    }
                    Ok(SyncAction::CreateBackup)
                }
                TimeComparison::LocalNewerWithinTolerance(delta) => {
                    if force {
                        return Ok(SyncAction::CreateBackup);
                    }
                    Ok(SyncAction::WithinTolerance {
                        direction: ToleranceDirection::LocalNewer,
                        delta_seconds: delta,
                    })
                }
                TimeComparison::SnapshotNewer => {
                    if !force && installation.checkpoint_matches(snapshot) {
                        return Ok(SyncAction::RestoreSkipped(snapshot.id.clone()));
                    }
                    Ok(SyncAction::RestoreFromSnapshot(snapshot.id.clone()))
                }
                TimeComparison::SnapshotNewerWithinTolerance(delta) => {
                    if force {
                        return Ok(SyncAction::RestoreFromSnapshot(snapshot.id.clone()));
                    }
                    if !force && installation.checkpoint_matches(snapshot) {
                        return Ok(SyncAction::RestoreSkipped(snapshot.id.clone()));
                    }
                    Ok(SyncAction::WithinTolerance {
                        direction: ToleranceDirection::SnapshotNewer,
                        delta_seconds: delta,
                    })
                }
                TimeComparison::Same => Ok(SyncAction::NoActionNeeded),
                TimeComparison::Error(e) => {
                    Ok(SyncAction::Error(format!("Time comparison error: {e}")))
                }
            }
        }
        (Some(_local_time), None) => {
            // Local saves exist but no snapshots - create initial backup
            Ok(SyncAction::CreateInitialBackup)
        }
        (None, Some(snapshot)) => {
            // Check if restore should be skipped due to matching checkpoint
            if !force && installation.checkpoint_matches(snapshot) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::config::PathContentKind;

    fn installation() -> GameInstallation {
        GameInstallation::with_kind(
            "Game",
            crate::common::TildePath::new(std::path::PathBuf::from("/nonexistent/saves")),
            PathContentKind::File,
        )
    }

    fn snapshot(id: &str) -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "time": "2026-02-01T00:00:00Z",
            "tree": "0".repeat(64),
            "paths": [],
            "hostname": "test-host",
            "username": "test-user",
            "tags": [],
            "id": id,
            "short_id": &id[..8],
        }))
        .unwrap()
    }

    fn time(value: &str) -> SystemTime {
        chrono::DateTime::parse_from_rfc3339(value).unwrap().into()
    }

    #[test]
    fn pending_restore_blocks_sync_before_missing_file_or_repository_access() {
        let mut installation = installation();
        installation.pending_restore = Some("historical".into());
        installation.sync_repository = Some("old-repo".into());
        for force in [false, true] {
            let error = determine_action(&installation, &InstantGameConfig::default(), force)
                .unwrap_err()
                .to_string();
            assert!(error.contains("incomplete"));
            assert!(error.contains("historical"));
            assert!(error.contains("ins game setup"));
        }
    }

    #[test]
    fn repository_mismatch_blocks_sync_before_missing_file_or_repository_access() {
        let mut installation = installation();
        installation.sync_repository = Some("old-repo".into());
        for force in [false, true] {
            let error = determine_action(&installation, &InstantGameConfig::default(), force)
                .unwrap_err()
                .to_string();
            assert!(error.contains("repository reconciliation"));
            assert!(error.contains("ins game setup"));
        }
    }

    #[test]
    fn acknowledged_head_preserves_historical_contents_until_local_edits() {
        let mut installation = installation();
        installation.nearest_checkpoint = Some("historical".into());
        installation.checkpoint_time = Some("2026-01-01T00:00:00Z".into());
        let head = snapshot(&"a".repeat(64));
        installation.acknowledged_snapshot = Some(head.id.clone());

        for local_time in [
            None,
            Some(time("2025-12-01T00:00:00Z")),
            Some(time("2026-01-01T00:00:00Z")),
        ] {
            assert_eq!(
                acknowledged_head_action(&installation, &head, local_time),
                Some(SyncAction::RestoreSkipped(head.id.clone()))
            );
        }
        let checkpoint_time = time("2026-01-01T00:00:00Z");
        let tolerance = std::time::Duration::from_secs(SYNC_TOLERANCE_SECONDS as u64);
        assert_eq!(
            acknowledged_head_action(&installation, &head, Some(checkpoint_time + tolerance)),
            Some(SyncAction::RestoreSkipped(head.id.clone()))
        );
        // Still older than the remote head: compare against the restore baseline, not head time.
        assert_eq!(
            acknowledged_head_action(
                &installation,
                &head,
                Some(checkpoint_time + tolerance + std::time::Duration::from_secs(1))
            ),
            Some(SyncAction::CreateBackup)
        );
        assert_eq!(
            installation.nearest_checkpoint.as_deref(),
            Some("historical")
        );
    }

    #[test]
    fn new_head_or_missing_checkpoint_resumes_normal_decision() {
        let mut installation = installation();
        let head = snapshot(&"a".repeat(64));
        installation.note_backup("historical");
        installation.acknowledged_snapshot = Some(head.short_id.clone());
        assert!(acknowledged_head_action(&installation, &head, None).is_some());
        assert!(
            acknowledged_head_action(&installation, &snapshot(&"b".repeat(64)), None).is_none()
        );
        installation.nearest_checkpoint = None;
        assert!(acknowledged_head_action(&installation, &head, None).is_none());
        installation.note_backup("historical");
        assert!(acknowledged_head_action(&installation, &head, None).is_none());
    }
}
