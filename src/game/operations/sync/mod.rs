mod decision;
mod execution;
mod types;
mod ui;

use crate::common::progress::create_spinner;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::utils::validation;
use anyhow::{Context, Result};
use types::SyncAction;

/// Summary of sync operation results
#[derive(Debug, Clone, Copy, Default)]
pub struct SyncSummary {
    pub synced: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl SyncSummary {
    pub fn total(&self) -> usize {
        self.synced + self.skipped + self.errors
    }

    pub fn is_success(&self) -> bool {
        self.errors == 0
    }
}

pub fn sync_game_saves(game_name: Option<String>, force: bool) -> Result<SyncSummary> {
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
        return Ok(SyncSummary::default());
    }

    let mut total_synced = 0;
    let mut total_skipped = 0;
    let mut total_errors = 0;

    // Sync each game
    for installation in games_to_sync {
        let game_name_plain = installation.game_name.0.clone();

        let spinner = create_spinner(format!("{}: Checking sync status...", game_name_plain));
        let action_result = decision::determine_action(&installation, &game_config, force);
        spinner.finish_and_clear();

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
                    let spinner =
                        create_spinner(format!("{}: Creating backup...", game_name_plain));
                    let result = execution::perform_backup(&installation, &game_config);
                    spinner.finish_and_clear();
                    ui::report_backup_result(&game_name_plain, &result);
                    if result.is_ok() {
                        total_synced += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                SyncAction::RestoreFromSnapshot(snapshot_id) => {
                    let spinner =
                        create_spinner(format!("{}: Restoring from snapshot...", game_name_plain));
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    spinner.finish_and_clear();
                    ui::report_restore_result(&game_name_plain, &snapshot_id, &result);
                    if result.is_ok() {
                        total_synced += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                SyncAction::RestoreFromLatest(snapshot_id) => {
                    let spinner =
                        create_spinner(format!("{}: Restoring latest backup...", game_name_plain));
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    spinner.finish_and_clear();
                    ui::report_restore_latest_result(&game_name_plain, &snapshot_id, &result);
                    if result.is_ok() {
                        total_synced += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                SyncAction::CreateInitialBackup => {
                    let spinner =
                        create_spinner(format!("{}: Creating initial backup...", game_name_plain));
                    let result = execution::perform_backup(&installation, &game_config);
                    spinner.finish_and_clear();
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

    let summary = SyncSummary {
        synced: total_synced as usize,
        skipped: total_skipped as usize,
        errors: total_errors as usize,
    };

    if total_errors > 0 {
        return Err(anyhow::anyhow!(
            "sync completed with {} errors",
            total_errors
        ))
        .with_context(|| format!("{:?}", summary));
    }

    Ok(summary)
}
