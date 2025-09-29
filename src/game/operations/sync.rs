use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::*;

use crate::game::checkpoint;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::restic::backup::GameBackup;
use crate::game::restic::cache;
use crate::game::utils::save_files::{TimeComparison, compare_snapshot_vs_local};
use crate::game::utils::validation;

/// Sync decision result
#[derive(Debug, PartialEq)]
enum SyncAction {
    /// No action needed (already in sync within tolerance)
    NoActionNeeded,
    /// Create backup (local saves are newer)
    CreateBackup,
    /// Restore from snapshot (snapshot is newer)
    RestoreFromSnapshot(String),
    /// No local saves, restore from latest snapshot
    RestoreFromLatest(String),
    /// No snapshots, create initial backup
    CreateInitialBackup,
    /// Restore skipped due to matching checkpoint
    RestoreSkipped(String),
    /// Error condition
    Error(String),
}

/// Handle game save synchronization
pub fn sync_game_saves(game_name: Option<String>, force: bool) -> Result<()> {
    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Check restic availability and game manager initialization
    validation::check_restic_and_game_manager(&game_config)?;

    // Determine which games to sync
    let games_to_sync = if let Some(name) = game_name {
        // Sync specific game
        match installations
            .installations
            .iter()
            .find(|inst| inst.game_name.0 == name)
        {
            Some(installation) => vec![installation.clone()],
            None => {
                error(
                    "game.sync.installation_missing",
                    &format!("Error: No installation found for game '{name}'."),
                );
                info(
                    "game.sync.hint.add",
                    &format!(
                        "Please add the game first using '{} game add'.",
                        env!("CARGO_BIN_NAME")
                    ),
                );
                return Err(anyhow::anyhow!("game installation not found"));
            }
        }
    } else {
        // Sync all games
        installations.installations.clone()
    };

    if games_to_sync.is_empty() {
        warn(
            "game.sync.none",
            "No games configured for syncing.",
        );
        info(
            "game.sync.hint.add",
            &format!("Add games using '{} game add'.", env!("CARGO_BIN_NAME")),
        );
        return Ok(());
    }

    let mut total_synced = 0;
    let mut total_skipped = 0;
    let mut total_errors = 0;

    // Sync each game
    for installation in games_to_sync {
        match sync_single_game(&installation, &game_config, force) {
            Ok(SyncAction::NoActionNeeded) => {
                success(
                    "game.sync.already_in_sync",
                    &format!("{}: Already in sync", installation.game_name.0.green()),
                );
                total_skipped += 1;
            }
            Ok(SyncAction::RestoreSkipped(snapshot_id)) => {
                info(
                    "game.sync.restore_skipped",
                    &format!(
                        "{}: Cloud checkpoint {} already matches your local saves (use --force to override)",
                        installation.game_name.0.yellow(),
                        snapshot_id
                    ),
                );
                total_skipped += 1;
            }
            Ok(SyncAction::CreateBackup) => {
                info(
                    "game.sync.backup.start",
                    &format!("{}: Creating backup...", installation.game_name.0.yellow()),
                );
                if let Err(e) = create_backup_for_game(&installation, &game_config) {
                    error(
                        "game.sync.backup.failed",
                        &format!("{}: Backup failed: {}", installation.game_name.0.red(), e),
                    );
                    total_errors += 1;
                } else {
                    success(
                        "game.sync.backup.completed",
                        &format!("{}: Backup completed", installation.game_name.0.green()),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::RestoreFromSnapshot(snapshot_id)) => {
                info(
                    "game.sync.restore.start",
                    &format!("{}: Restoring from snapshot...", installation.game_name.0.yellow()),
                );
                if let Err(e) =
                    restore_game_from_snapshot(&installation, &game_config, &snapshot_id)
                {
                    error(
                        "game.sync.restore.failed",
                        &format!("{}: Restore failed: {}", installation.game_name.0.red(), e),
                    );
                    total_errors += 1;
                } else {
                    success(
                        "game.sync.restore.completed",
                        &format!("{}: Restore completed", installation.game_name.0.green()),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::RestoreFromLatest(snapshot_id)) => {
                info(
                    "game.sync.restore.latest.start",
                    &format!("{}: No local saves, restoring from latest snapshot...", installation.game_name.0.yellow()),
                );
                if let Err(e) =
                    restore_game_from_snapshot(&installation, &game_config, &snapshot_id)
                {
                    error(
                        "game.sync.restore.latest.failed",
                        &format!("{}: Restore failed: {}", installation.game_name.0.red(), e),
                    );
                    total_errors += 1;
                } else {
                    success(
                        "game.sync.restore.latest.completed",
                        &format!("{}: Restore completed", installation.game_name.0.green()),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::CreateInitialBackup) => {
                info(
                    "game.sync.initial_backup.start",
                    &format!("{}: No snapshots found, creating initial backup...", installation.game_name.0.yellow()),
                );
                if let Err(e) = create_backup_for_game(&installation, &game_config) {
                    error(
                        "game.sync.initial_backup.failed",
                        &format!("{}: Initial backup failed: {}", installation.game_name.0.red(), e),
                    );
                    total_errors += 1;
                } else {
                    success(
                        "game.sync.initial_backup.completed",
                        &format!("{}: Initial backup completed", installation.game_name.0.green()),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::Error(msg)) => {
                error(
                    "game.sync.error",
                    &format!("{}: {}", installation.game_name.0.red(), msg),
                );
                total_errors += 1;
            }
            Err(e) => {
                error(
                    "game.sync.failed",
                    &format!("{}: Sync failed: {}", installation.game_name.0.red(), e),
                );
                total_errors += 1;
            }
        }
    }

    // Print summary
    separator(true);
    info("game.sync.summary.title", "Sync Summary:");
    let summary_text = format!(
        "  Synced: {}\n  Skipped: {}\n  Errors: {}",
        total_synced.to_string().green(),
        total_skipped.to_string().yellow(),
        total_errors.to_string().red()
    );
    emit(
        Level::Info,
        "game.sync.summary",
        &summary_text,
        Some(serde_json::json!({
            "synced": total_synced,
            "skipped": total_skipped,
            "errors": total_errors
        })),
    );
    separator(true);

    if total_errors > 0 {
        return Err(anyhow::anyhow!(
            "sync completed with {} errors",
            total_errors
        ));
    }

    Ok(())
}

/// Sync a single game and determine the required action
fn sync_single_game(
    installation: &crate::game::config::GameInstallation,
    game_config: &InstantGameConfig,
    force: bool,
) -> Result<SyncAction> {
    let game_name = &installation.game_name.0;
    let save_path = installation.save_path.as_path();

    // Security check: ensure save directory exists
    if !save_path.exists() {
        return Ok(SyncAction::Error(format!(
            "Save path does not exist: {}",
            save_path.display()
        )));
    }

    // Get local save information
    let local_save_info = crate::game::utils::save_files::get_save_directory_info(save_path)?;

    // Security check: ensure save directory is not empty before backing up
    if local_save_info.file_count == 0 {
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
                Ok(TimeComparison::LocalNewer) => Ok(SyncAction::CreateBackup),
                Ok(TimeComparison::SnapshotNewer) => {
                    if !force
                        && let Some(ref nearest_checkpoint) = installation.nearest_checkpoint
                        && nearest_checkpoint == &snapshot.id
                    {
                        return Ok(SyncAction::RestoreSkipped(snapshot.id.clone()));
                    }
                    Ok(SyncAction::RestoreFromSnapshot(snapshot.id.clone()))
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

/// Create backup for a game
fn create_backup_for_game(
    installation: &crate::game::config::GameInstallation,
    game_config: &InstantGameConfig,
) -> Result<()> {
    let backup_handler = GameBackup::new(game_config.clone());

    let backup_result = backup_handler
        .backup_game(installation)
        .context("Failed to create backup")?;

    // Update checkpoint after successful backup
    checkpoint::update_checkpoint_after_backup(
        &backup_result,
        &installation.game_name.0,
        game_config,
    )?;

    // Invalidate cache for this game after successful backup
    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(&installation.game_name.0, &repo_path);

    Ok(())
}

/// Restore game from snapshot
fn restore_game_from_snapshot(
    installation: &crate::game::config::GameInstallation,
    game_config: &InstantGameConfig,
    snapshot_id: &str,
) -> Result<()> {
    let backup_handler = GameBackup::new(game_config.clone());
    let save_path = installation.save_path.as_path();

    backup_handler
        .restore_game_backup(&installation.game_name.0, snapshot_id, save_path)
        .context("Failed to restore from snapshot")?;

    // Update the installation with the checkpoint
    checkpoint::update_checkpoint_after_restore(&installation.game_name.0, snapshot_id)?;

    // Invalidate cache for this game after successful restore
    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(&installation.game_name.0, &repo_path);

    Ok(())
}
