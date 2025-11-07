use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::*;

use crate::game::checkpoint;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::restic::backup::GameBackup;
use crate::game::restic::cache;
use crate::game::utils::save_files::{
    SYNC_TOLERANCE_SECONDS, TimeComparison, compare_snapshot_vs_local,
};
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
    /// Skipped due to being within tolerance window
    WithinTolerance {
        direction: ToleranceDirection,
        delta_seconds: i64,
    },
    /// Error condition
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToleranceDirection {
    LocalNewer,
    SnapshotNewer,
}

fn emit_with_icon(
    level: Level,
    code: &str,
    icon: char,
    plain_message: impl Into<String>,
    text_message: impl Into<String>,
    data: Option<serde_json::Value>,
) {
    let plain = plain_message.into();
    let text = text_message.into();
    let formatted = if matches!(get_output_format(), OutputFormat::Json) {
        plain
    } else {
        format!("{icon} {text}")
    };
    emit(level, code, &formatted, data);
}

fn emit_separator() {
    let ch = if matches!(get_output_format(), OutputFormat::Json) {
        '-'
    } else {
        '─'
    };
    let line: String = std::iter::repeat_n(ch, 80).collect();
    emit(Level::Info, "separator", &line, None);
}

fn format_delta(delta_seconds: i64) -> String {
    let secs = delta_seconds.unsigned_abs();
    if secs < 60 {
        format!("{} seconds", secs)
    } else {
        let minutes = secs / 60;
        let seconds = secs % 60;
        if seconds == 0 {
            format!("{} minutes", minutes)
        } else {
            format!("{} minutes {} seconds", minutes, seconds)
        }
    }
}

fn format_tolerance_window() -> String {
    let secs = SYNC_TOLERANCE_SECONDS.unsigned_abs();
    if secs < 60 {
        format!("{} seconds", secs)
    } else {
        let minutes = secs / 60;
        let seconds = secs % 60;
        if seconds == 0 {
            format!("{} minutes", minutes)
        } else {
            format!("{} minutes {} seconds", minutes, seconds)
        }
    }
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
                emit_with_icon(
                    Level::Error,
                    "game.sync.installation_missing",
                    char::from(NerdFont::CrossCircle),
                    format!("Error: No installation found for game '{name}'."),
                    format!("Error: No installation found for game '{}'.", name.red()),
                    Some(serde_json::json!({
                        "game": name,
                        "action": "installation_missing"
                    })),
                );
                emit_with_icon(
                    Level::Info,
                    "game.sync.hint.add",
                    char::from(NerdFont::Info),
                    format!(
                        "Please add the game first using '{} game add'.",
                        env!("CARGO_BIN_NAME")
                    ),
                    format!(
                        "Please add the game first using '{} game add'.",
                        env!("CARGO_BIN_NAME")
                    ),
                    Some(serde_json::json!({
                        "hint": "add_game",
                        "command": format!("{} game add", env!("CARGO_BIN_NAME"))
                    })),
                );
                return Err(anyhow::anyhow!("game installation not found"));
            }
        }
    } else {
        // Sync all games
        installations.installations.clone()
    };

    if games_to_sync.is_empty() {
        emit_with_icon(
            Level::Warn,
            "game.sync.none",
            char::from(NerdFont::Warning),
            "No games configured for syncing.".to_string(),
            "No games configured for syncing.".to_string(),
            Some(serde_json::json!({
                "action": "no_games"
            })),
        );
        emit_with_icon(
            Level::Info,
            "game.sync.hint.add",
            char::from(NerdFont::Info),
            format!("Add games using '{} game add'.", env!("CARGO_BIN_NAME")),
            format!("Add games using '{} game add'.", env!("CARGO_BIN_NAME")),
            Some(serde_json::json!({
                "hint": "add_game",
                "command": format!("{} game add", env!("CARGO_BIN_NAME"))
            })),
        );
        return Ok(());
    }

    let mut total_synced = 0;
    let mut total_skipped = 0;
    let mut total_errors = 0;

    // Sync each game
    for installation in games_to_sync {
        let game_name_plain = installation.game_name.0.clone();
        match sync_single_game(&installation, &game_config, force) {
            Ok(SyncAction::NoActionNeeded) => {
                emit_with_icon(
                    Level::Success,
                    "game.sync.already_in_sync",
                    char::from(NerdFont::Check),
                    format!("{}: Already in sync", game_name_plain),
                    format!("{}: Already in sync", installation.game_name.0.green()),
                    Some(serde_json::json!({
                        "game": game_name_plain,
                        "action": "already_in_sync"
                    })),
                );
                total_skipped += 1;
            }
            Ok(SyncAction::WithinTolerance {
                direction,
                delta_seconds,
            }) => {
                let delta_str = format_delta(delta_seconds);
                let tolerance_str = format_tolerance_window();
                let (plain_msg, text_msg, code, direction_value) = match direction {
                    ToleranceDirection::LocalNewer => (
                        format!(
                            "{}: Local saves are newer by {}, within the {} safety window (use --force to back up immediately)",
                            game_name_plain, delta_str, tolerance_str
                        ),
                        format!(
                            "{}: Local saves are newer by {} within the {} safety window (use --force to back up immediately)",
                            installation.game_name.0.yellow(),
                            delta_str,
                            tolerance_str
                        ),
                        "game.sync.within_tolerance.local_newer",
                        "local_newer",
                    ),
                    ToleranceDirection::SnapshotNewer => (
                        format!(
                            "{}: Latest snapshot is newer by {}, within the {} safety window (use --force to restore immediately)",
                            game_name_plain, delta_str, tolerance_str
                        ),
                        format!(
                            "{}: Latest snapshot is newer by {} within the {} safety window (use --force to restore immediately)",
                            installation.game_name.0.yellow(),
                            delta_str,
                            tolerance_str
                        ),
                        "game.sync.within_tolerance.snapshot_newer",
                        "snapshot_newer",
                    ),
                };

                emit_with_icon(
                    Level::Info,
                    code,
                    char::from(NerdFont::Info),
                    plain_msg,
                    text_msg,
                    Some(serde_json::json!({
                        "game": game_name_plain,
                        "action": "within_tolerance",
                        "direction": direction_value,
                        "delta_seconds": delta_seconds,
                        "tolerance_window_seconds": SYNC_TOLERANCE_SECONDS,
                    })),
                );
                total_skipped += 1;
            }
            Ok(SyncAction::RestoreSkipped(snapshot_id)) => {
                emit_with_icon(
                    Level::Info,
                    "game.sync.restore_skipped",
                    char::from(NerdFont::Info),
                    format!(
                        "{}: Cloud checkpoint {} already matches your local saves (use --force to override)",
                        game_name_plain, snapshot_id
                    ),
                    format!(
                        "{}: Cloud checkpoint {} already matches your local saves (use --force to override)",
                        installation.game_name.0.yellow(),
                        snapshot_id
                    ),
                    Some(serde_json::json!({
                        "game": game_name_plain,
                        "action": "restore_skipped",
                        "snapshot_id": snapshot_id
                    })),
                );
                total_skipped += 1;
            }
            Ok(SyncAction::CreateBackup) => {
                emit_with_icon(
                    Level::Info,
                    "game.sync.backup.start",
                    char::from(NerdFont::Upload),
                    format!("{}: Creating backup...", game_name_plain.clone()),
                    format!("{}: Creating backup...", installation.game_name.0.yellow()),
                    Some(serde_json::json!({
                        "game": game_name_plain.clone(),
                        "action": "backup_start"
                    })),
                );
                if let Err(e) = create_backup_for_game(&installation, &game_config) {
                    emit_with_icon(
                        Level::Error,
                        "game.sync.backup.failed",
                        char::from(NerdFont::CrossCircle),
                        format!("{}: Backup failed: {}", game_name_plain.clone(), e),
                        format!("{}: Backup failed: {}", installation.game_name.0.red(), e),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "backup_failed",
                            "error": e.to_string()
                        })),
                    );
                    total_errors += 1;
                } else {
                    emit_with_icon(
                        Level::Success,
                        "game.sync.backup.completed",
                        char::from(NerdFont::Check),
                        format!("{}: Backup completed", game_name_plain.clone()),
                        format!("{}: Backup completed", installation.game_name.0.green()),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "backup_completed"
                        })),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::RestoreFromSnapshot(snapshot_id)) => {
                emit_with_icon(
                    Level::Info,
                    "game.sync.restore.start",
                    char::from(NerdFont::Info),
                    format!("{}: Restoring from snapshot...", game_name_plain.clone()),
                    format!(
                        "{}: Restoring from snapshot...",
                        installation.game_name.0.yellow()
                    ),
                    Some(serde_json::json!({
                        "game": game_name_plain.clone(),
                        "action": "restore_start",
                        "snapshot_id": snapshot_id
                    })),
                );
                if let Err(e) =
                    restore_game_from_snapshot(&installation, &game_config, &snapshot_id)
                {
                    emit_with_icon(
                        Level::Error,
                        "game.sync.restore.failed",
                        char::from(NerdFont::CrossCircle),
                        format!("{}: Restore failed: {}", game_name_plain.clone(), e),
                        format!("{}: Restore failed: {}", installation.game_name.0.red(), e),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "restore_failed",
                            "snapshot_id": snapshot_id,
                            "error": e.to_string()
                        })),
                    );
                    total_errors += 1;
                } else {
                    emit_with_icon(
                        Level::Success,
                        "game.sync.restore.completed",
                        char::from(NerdFont::Check),
                        format!("{}: Restore completed", game_name_plain.clone()),
                        format!("{}: Restore completed", installation.game_name.0.green()),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "restore_completed",
                            "snapshot_id": snapshot_id
                        })),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::RestoreFromLatest(snapshot_id)) => {
                emit_with_icon(
                    Level::Info,
                    "game.sync.restore.latest.start",
                    char::from(NerdFont::Info),
                    format!(
                        "{}: No local saves, restoring from latest snapshot...",
                        game_name_plain.clone()
                    ),
                    format!(
                        "{}: No local saves, restoring from latest snapshot...",
                        installation.game_name.0.yellow()
                    ),
                    Some(serde_json::json!({
                        "game": game_name_plain.clone(),
                        "action": "restore_latest_start",
                        "snapshot_id": snapshot_id
                    })),
                );
                if let Err(e) =
                    restore_game_from_snapshot(&installation, &game_config, &snapshot_id)
                {
                    emit_with_icon(
                        Level::Error,
                        "game.sync.restore.latest.failed",
                        char::from(NerdFont::CrossCircle),
                        format!("{}: Restore failed: {}", game_name_plain.clone(), e),
                        format!("{}: Restore failed: {}", installation.game_name.0.red(), e),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "restore_latest_failed",
                            "snapshot_id": snapshot_id,
                            "error": e.to_string()
                        })),
                    );
                    total_errors += 1;
                } else {
                    emit_with_icon(
                        Level::Success,
                        "game.sync.restore.latest.completed",
                        char::from(NerdFont::Check),
                        format!("{}: Restore completed", game_name_plain.clone()),
                        format!("{}: Restore completed", installation.game_name.0.green()),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "restore_latest_completed",
                            "snapshot_id": snapshot_id
                        })),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::CreateInitialBackup) => {
                emit_with_icon(
                    Level::Info,
                    "game.sync.initial_backup.start",
                    char::from(NerdFont::Upload),
                    format!(
                        "{}: No snapshots found, creating initial backup...",
                        game_name_plain.clone()
                    ),
                    format!(
                        "{}: No snapshots found, creating initial backup...",
                        installation.game_name.0.yellow()
                    ),
                    Some(serde_json::json!({
                        "game": game_name_plain.clone(),
                        "action": "initial_backup_start"
                    })),
                );
                if let Err(e) = create_backup_for_game(&installation, &game_config) {
                    emit_with_icon(
                        Level::Error,
                        "game.sync.initial_backup.failed",
                        char::from(NerdFont::CrossCircle),
                        format!("{}: Initial backup failed: {}", game_name_plain.clone(), e),
                        format!(
                            "{}: Initial backup failed: {}",
                            installation.game_name.0.red(),
                            e
                        ),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "initial_backup_failed",
                            "error": e.to_string()
                        })),
                    );
                    total_errors += 1;
                } else {
                    emit_with_icon(
                        Level::Success,
                        "game.sync.initial_backup.completed",
                        char::from(NerdFont::Check),
                        format!("{}: Initial backup completed", game_name_plain.clone()),
                        format!(
                            "{}: Initial backup completed",
                            installation.game_name.0.green()
                        ),
                        Some(serde_json::json!({
                            "game": game_name_plain.clone(),
                            "action": "initial_backup_completed"
                        })),
                    );
                    total_synced += 1;
                }
            }
            Ok(SyncAction::Error(msg)) => {
                emit_with_icon(
                    Level::Error,
                    "game.sync.error",
                    char::from(NerdFont::CrossCircle),
                    format!("{}: {}", game_name_plain.clone(), msg),
                    format!("{}: {}", installation.game_name.0.red(), msg),
                    Some(serde_json::json!({
                        "game": game_name_plain.clone(),
                        "action": "error",
                        "message": msg
                    })),
                );
                total_errors += 1;
            }
            Err(e) => {
                emit_with_icon(
                    Level::Error,
                    "game.sync.failed",
                    char::from(NerdFont::CrossCircle),
                    format!("{}: Sync failed: {}", game_name_plain.clone(), e),
                    format!("{}: Sync failed: {}", installation.game_name.0.red(), e),
                    Some(serde_json::json!({
                        "game": game_name_plain,
                        "action": "sync_failed",
                        "error": e.to_string()
                    })),
                );
                total_errors += 1;
            }
        }
    }

    // Print summary
    emit_separator();
    let summary_data = serde_json::json!({
        "synced": total_synced,
        "skipped": total_skipped,
        "errors": total_errors
    });

    let summary_title = if matches!(get_output_format(), OutputFormat::Json) {
        "Sync Summary".to_string()
    } else {
        format!(
            "{} {} Sync Summary",
            char::from(NerdFont::Chart),
            char::from(NerdFont::List)
        )
    };

    if matches!(get_output_format(), OutputFormat::Json) {
        emit(Level::Info, "game.sync.summary.title", &summary_title, None);
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
        emit_separator();
    } else {
        emit(
            Level::Info,
            "game.sync.summary.title",
            &summary_title,
            Some(summary_data),
        );

        let entries = [
            (
                Level::Success,
                Some(char::from(NerdFont::Check)),
                "Synced",
                total_synced,
                "game.sync.summary.synced",
            ),
            (
                Level::Info,
                Some(char::from(NerdFont::Flag)),
                "Skipped",
                total_skipped,
                "game.sync.summary.skipped",
            ),
            (
                Level::Error,
                Some(char::from(NerdFont::CrossCircle)),
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
            let label_with_icon = if matches!(get_output_format(), OutputFormat::Json) {
                format!("{label}:")
            } else {
                match icon {
                    Some(icon) => format!("{icon} {label}:"),
                    None => format!("  {label}:"),
                }
            };
            let padded_label = format!("{label_with_icon:<width$}", width = column_width);
            let message = format!("{padded_label} {value}");
            emit(
                level,
                code,
                &message,
                Some(serde_json::json!({
                    "label": label.to_lowercase(),
                    "count": value
                })),
            );
        }

        emit_separator();
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
    let local_save_info = crate::game::utils::save_files::get_save_directory_info(save_path)?;

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
    let snapshot_hint =
        cache::get_snapshot_by_id(snapshot_id, &installation.game_name.0, game_config)
            .ok()
            .and_then(|snapshot| snapshot.and_then(|snap| snap.paths.first().cloned()));

    // Use the appropriate restore method based on save path type
    backup_handler
        .restore_backup(
            &installation.game_name.0,
            snapshot_id,
            save_path,
            installation.save_path_type,
            save_path,
            snapshot_hint.as_deref(),
        )
        .context("Failed to restore from snapshot")?;

    // Update the installation with the checkpoint
    checkpoint::update_checkpoint_after_restore(&installation.game_name.0, snapshot_id)?;

    // Invalidate cache for this game after successful restore
    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(&installation.game_name.0, &repo_path);

    Ok(())
}
