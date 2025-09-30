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
                emit(
                    Level::Error,
                    "game.sync.installation_missing",
                    &format!(
                        "{} Error: No installation found for game '{name}'.",
                        char::from(Fa::TimesCircle)
                    ),
                    None,
                );
                emit(
                    Level::Info,
                    "game.sync.hint.add",
                    &format!(
                        "{} Please add the game first using '{} game add'.",
                        char::from(Fa::InfoCircle),
                        env!("CARGO_BIN_NAME")
                    ),
                    None,
                );
                return Err(anyhow::anyhow!("game installation not found"));
            }
        }
    } else {
        // Sync all games
        installations.installations.clone()
    };

    if games_to_sync.is_empty() {
        emit(
            Level::Warn,
            "game.sync.none",
            &format!(
                "{} No games configured for syncing.",
                char::from(Fa::ExclamationCircle)
            ),
            None,
        );
        emit(
            Level::Info,
            "game.sync.hint.add",
            &format!(
                "{} Add games using '{} game add'.",
                char::from(Fa::InfoCircle),
                env!("CARGO_BIN_NAME")
            ),
            None,
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
                emit(
                    Level::Success,
                    "game.sync.already_in_sync",
                    &format!(
                        "{} {}: Already in sync",
                        char::from(Fa::Check),
                        installation.game_name.0.green()
                    ),
                    None,
                );
                total_skipped += 1;
            }
            Ok(SyncAction::RestoreSkipped(snapshot_id)) => {
                emit(
                    Level::Info,
                    "game.sync.restore_skipped",
                    &format!(
                        "{} {}: Cloud checkpoint {} already matches your local saves (use --force to override)",
                        char::from(Fa::InfoCircle),
                        installation.game_name.0.yellow(),
                        snapshot_id
                    ),
                    None,
                );
                total_skipped += 1;
            }
            Ok(SyncAction::CreateBackup) => {
                emit(
                    Level::Info,
                    "game.sync.backup.start",
                    &format!(
                        "{} {}: Creating backup...",
                        char::from(Fa::Upload),
                        installation.game_name.0.yellow()
                    ),
                    None,
                );
                if let Err(e) = create_backup_for_game(&installation, &game_config) {
                    emit(
                        Level::Error,
                        "game.sync.backup.failed",
                        &format!(
                            "{} {}: Backup failed: {}",
                            char::from(Fa::TimesCircle),
                            installation.game_name.0.red(),
                            e
                        ),
                        None,
                    );
                    total_errors += 1;
                } else {
                    emit(
                        Level::Success,
                        "game.sync.backup.completed",
                        &format!(
                            "{} {}: Backup completed",
                            char::from(Fa::Check),
                            installation.game_name.0.green()
                        ),
                        None,
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::RestoreFromSnapshot(snapshot_id)) => {
                emit(
                    Level::Info,
                    "game.sync.restore.start",
                    &format!(
                        "{} {}: Restoring from snapshot...",
                        char::from(Fa::InfoCircle),
                        installation.game_name.0.yellow()
                    ),
                    None,
                );
                if let Err(e) =
                    restore_game_from_snapshot(&installation, &game_config, &snapshot_id)
                {
                    emit(
                        Level::Error,
                        "game.sync.restore.failed",
                        &format!(
                            "{} {}: Restore failed: {}",
                            char::from(Fa::TimesCircle),
                            installation.game_name.0.red(),
                            e
                        ),
                        None,
                    );
                    total_errors += 1;
                } else {
                    emit(
                        Level::Success,
                        "game.sync.restore.completed",
                        &format!(
                            "{} {}: Restore completed",
                            char::from(Fa::Check),
                            installation.game_name.0.green()
                        ),
                        None,
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::RestoreFromLatest(snapshot_id)) => {
                emit(
                    Level::Info,
                    "game.sync.restore.latest.start",
                    &format!(
                        "{} {}: No local saves, restoring from latest snapshot...",
                        char::from(Fa::InfoCircle),
                        installation.game_name.0.yellow()
                    ),
                    None,
                );
                if let Err(e) =
                    restore_game_from_snapshot(&installation, &game_config, &snapshot_id)
                {
                    emit(
                        Level::Error,
                        "game.sync.restore.latest.failed",
                        &format!(
                            "{} {}: Restore failed: {}",
                            char::from(Fa::TimesCircle),
                            installation.game_name.0.red(),
                            e
                        ),
                        None,
                    );
                    total_errors += 1;
                } else {
                    emit(
                        Level::Success,
                        "game.sync.restore.latest.completed",
                        &format!(
                            "{} {}: Restore completed",
                            char::from(Fa::Check),
                            installation.game_name.0.green()
                        ),
                        None,
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::CreateInitialBackup) => {
                emit(
                    Level::Info,
                    "game.sync.initial_backup.start",
                    &format!(
                        "{} {}: No snapshots found, creating initial backup...",
                        char::from(Fa::Upload),
                        installation.game_name.0.yellow()
                    ),
                    None,
                );
                if let Err(e) = create_backup_for_game(&installation, &game_config) {
                    emit(
                        Level::Error,
                        "game.sync.initial_backup.failed",
                        &format!(
                            "{} {}: Initial backup failed: {}",
                            char::from(Fa::TimesCircle),
                            installation.game_name.0.red(),
                            e
                        ),
                        None,
                    );
                    total_errors += 1;
                } else {
                    emit(
                        Level::Success,
                        "game.sync.initial_backup.completed",
                        &format!(
                            "{} {}: Initial backup completed",
                            char::from(Fa::Check),
                            installation.game_name.0.green()
                        ),
                        None,
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::Error(msg)) => {
                emit(
                    Level::Error,
                    "game.sync.error",
                    &format!(
                        "{} {}: {}",
                        char::from(Fa::TimesCircle),
                        installation.game_name.0.red(),
                        msg
                    ),
                    None,
                );
                total_errors += 1;
            }
            Err(e) => {
                emit(
                    Level::Error,
                    "game.sync.failed",
                    &format!(
                        "{} {}: Sync failed: {}",
                        char::from(Fa::TimesCircle),
                        installation.game_name.0.red(),
                        e
                    ),
                    None,
                );
                total_errors += 1;
            }
        }
    }

    // Print summary
    emit(Level::Info, "separator", &"─".repeat(80), None);
    let summary_data = serde_json::json!({
        "synced": total_synced,
        "skipped": total_skipped,
        "errors": total_errors
    });

    if matches!(get_output_format(), OutputFormat::Json) {
        emit(
            Level::Info,
            "game.sync.summary.title",
            &format!("📊 {} Sync Summary", char::from(Fa::List)),
            None,
        );
        //TODO: add nerd font icons
        let summary_text = format!(
            "  Synced: {}\n  Skipped: {}\n  Errors: {}",
            total_synced, total_skipped, total_errors
        );
        emit(
            Level::Info,
            "game.sync.summary",
            &summary_text,
            Some(summary_data),
        );
        emit(Level::Info, "separator", &"─".repeat(80), None);
    } else {
        emit(
            Level::Info,
            "game.sync.summary.title",
            &format!("📊 {} Sync Summary", char::from(Fa::List)),
            Some(summary_data),
        );

        let entries = [
            (
                Level::Success,
                Some(char::from(Fa::Check)),
                "Synced",
                total_synced,
                "game.sync.summary.synced",
            ),
            (
                Level::Info,
                Some(char::from(Fa::Flag)),
                "Skipped",
                total_skipped,
                "game.sync.summary.skipped",
            ),
            (
                Level::Error,
                Some(char::from(Fa::TimesCircle)),
                "Errors",
                total_errors,
                "game.sync.summary.errors",
            ),
        ];

        let label_width = entries
            .iter()
            .map(|(_, _, label, _, _)| label.len())
            .max()
            .unwrap_or(0);
        let column_width = label_width + 4;

        for (level, icon, label, value, code) in entries {
            let label_with_icon = match icon {
                Some(icon) => format!("{icon} {label}:"),
                None => format!("  {label}:")
            };
            let padded_label = format!("{label_with_icon:<width$}", width = column_width);
            let message = format!("{padded_label} {value}");
            emit(level, code, &message, None);
        }

        emit(Level::Info, "separator", &"─".repeat(80), None);
    }

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
