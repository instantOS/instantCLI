mod decision;
mod execution;
mod types;
mod ui;

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::utils::validation;
use anyhow::{Context, Result};
use types::SyncAction;

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
                ui::report_installation_missing(&name);
                return Err(anyhow::anyhow!("game installation not found"));
            }
        }
    } else {
        // Sync all games
        installations.installations.clone()
    };

    if games_to_sync.is_empty() {
        ui::report_no_games_configured();
        return Ok(());
    }

    let mut total_synced = 0;
    let mut total_skipped = 0;
    let mut total_errors = 0;

    // Sync each game
    for installation in games_to_sync {
        let game_name_plain = installation.game_name.0.clone();

        let action_result = decision::determine_action(&installation, &game_config, force);

        match action_result {
            Ok(action) => match action {
                SyncAction::NoActionNeeded => {
                    ui::report_no_action_needed(&game_name_plain);
                    total_skipped += 1;
                }
                SyncAction::WithinTolerance {
                    direction,
                    delta_seconds,
                } => {
                    ui::report_within_tolerance(&game_name_plain, direction, delta_seconds);
                    total_skipped += 1;
                }
                SyncAction::RestoreSkipped(snapshot_id) => {
                    ui::report_restore_skipped(&game_name_plain, &snapshot_id);
                    total_skipped += 1;
                }
                SyncAction::BackupSkipped(snapshot_id) => {
                    ui::report_backup_skipped(&game_name_plain, &snapshot_id);
                    total_skipped += 1;
                }
                SyncAction::CreateBackup => {
                    ui::report_backup_start(&game_name_plain);
                    let result = execution::perform_backup(&installation, &game_config);
                    ui::report_backup_result(&game_name_plain, &result);
                    if result.is_ok() {
                        total_synced += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                SyncAction::RestoreFromSnapshot(snapshot_id) => {
                    ui::report_restore_start(&game_name_plain, &snapshot_id);
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    ui::report_restore_result(&game_name_plain, &snapshot_id, &result);
                    if result.is_ok() {
                        total_synced += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                SyncAction::RestoreFromLatest(snapshot_id) => {
                    ui::report_restore_latest_start(&game_name_plain, &snapshot_id);
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    ui::report_restore_latest_result(&game_name_plain, &snapshot_id, &result);
                    if result.is_ok() {
                        total_synced += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                SyncAction::CreateInitialBackup => {
                    ui::report_initial_backup_start(&game_name_plain);
                    let result = execution::perform_backup(&installation, &game_config);
                    ui::report_initial_backup_result(&game_name_plain, &result);
                    if result.is_ok() {
                        total_synced += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                SyncAction::Error(msg) => {
                    ui::report_error(&game_name_plain, &msg);
                    total_errors += 1;
                }
            },
            Err(e) => {
                ui::report_sync_failure(&game_name_plain, &e);
                total_errors += 1;
            }
        }
    }

    // Print summary
    ui::report_summary(total_synced, total_skipped, total_errors);

    if total_errors > 0 {
        return Err(anyhow::anyhow!(
            "sync completed with {} errors",
            total_errors
        ));
    }

    Ok(())
}
