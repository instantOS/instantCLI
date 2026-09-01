mod decision;
mod execution;
mod types;
mod ui;

use crate::common::progress::create_spinner;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::utils::validation;
use anyhow::{Context, Result};
use types::{GameSyncOutcome, GameSyncStatus, SyncAction};

/// Summary of sync operation results
#[derive(Debug, Clone, Copy, Default)]
pub struct SyncSummary {
    pub backed_up: usize,
    pub restored: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl SyncSummary {
    pub fn total(&self) -> usize {
        self.backed_up + self.restored + self.skipped + self.errors
    }
}

/// Results of a sync run: aggregate counts plus per-game outcomes.
///
/// Per-game failures are recorded here instead of failing the whole batch,
/// so callers can decide whether a specific game's failure is fatal.
#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub summary: SyncSummary,
    pub games: Vec<GameSyncOutcome>,
}

impl SyncReport {
    /// Error message if the given game's sync failed
    pub fn failure_for(&self, game_name: &str) -> Option<&str> {
        self.games
            .iter()
            .find(|outcome| outcome.game == game_name)
            .and_then(|outcome| match &outcome.status {
                GameSyncStatus::Failed(message) => Some(message.as_str()),
                _ => None,
            })
    }
}

/// Sync game saves and report per-game outcomes.
///
/// Individual game failures (e.g. a missing save path on an unmounted drive)
/// are recorded in the returned [`SyncReport`] instead of failing the whole
/// batch; callers decide whether a specific failure is fatal. Only global
/// failures (config load, restic availability) return `Err`.
pub fn sync_game_saves(game_name: Option<String>, force: bool) -> Result<SyncReport> {
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
        return Ok(SyncReport::default());
    }

    let mut report = SyncReport::default();

    // Sync each game
    for installation in games_to_sync {
        let game_name_plain = installation.game_name.0.clone();

        let spinner = create_spinner(format!("{}: Checking sync status...", game_name_plain));
        let action_result = decision::determine_action(&installation, &game_config, force);
        spinner.finish_and_clear();

        let status = match action_result {
            Ok(action) => match action {
                SyncAction::NoActionNeeded => {
                    ui::report_no_action_needed(&game_name_plain);
                    GameSyncStatus::Skipped
                }
                SyncAction::WithinTolerance {
                    direction,
                    delta_seconds,
                } => {
                    ui::report_within_tolerance(&game_name_plain, direction, delta_seconds);
                    GameSyncStatus::Skipped
                }
                SyncAction::RestoreSkipped(snapshot_id) => {
                    ui::report_restore_skipped(&game_name_plain, &snapshot_id);
                    GameSyncStatus::Skipped
                }
                SyncAction::BackupSkipped(snapshot_id) => {
                    ui::report_backup_skipped(&game_name_plain, &snapshot_id);
                    GameSyncStatus::Skipped
                }
                SyncAction::CreateBackup => {
                    let spinner =
                        create_spinner(format!("{}: Creating backup...", game_name_plain));
                    let result = execution::perform_backup(&installation, &game_config);
                    spinner.finish_and_clear();
                    ui::report_backup_result(&game_name_plain, &result);
                    sync_status(result, GameSyncStatus::BackedUp)
                }
                SyncAction::RestoreFromSnapshot(snapshot_id) => {
                    let spinner =
                        create_spinner(format!("{}: Restoring from snapshot...", game_name_plain));
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    spinner.finish_and_clear();
                    ui::report_restore_result(&game_name_plain, &snapshot_id, &result);
                    sync_status(result, GameSyncStatus::Restored)
                }
                SyncAction::RestoreFromLatest(snapshot_id) => {
                    let spinner =
                        create_spinner(format!("{}: Restoring latest backup...", game_name_plain));
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    spinner.finish_and_clear();
                    ui::report_restore_latest_result(&game_name_plain, &snapshot_id, &result);
                    sync_status(result, GameSyncStatus::Restored)
                }
                SyncAction::CreateInitialBackup => {
                    let spinner =
                        create_spinner(format!("{}: Creating initial backup...", game_name_plain));
                    let result = execution::perform_backup(&installation, &game_config);
                    spinner.finish_and_clear();
                    ui::report_initial_backup_result(&game_name_plain, &result);
                    sync_status(result, GameSyncStatus::BackedUp)
                }
                SyncAction::Error(msg) => {
                    ui::report_error(&game_name_plain, &msg);
                    GameSyncStatus::Failed(msg)
                }
            },
            Err(e) => {
                ui::report_sync_failure(&game_name_plain, &e);
                GameSyncStatus::Failed(e.to_string())
            }
        };

        match status {
            GameSyncStatus::BackedUp => report.summary.backed_up += 1,
            GameSyncStatus::Restored => report.summary.restored += 1,
            GameSyncStatus::Skipped => report.summary.skipped += 1,
            GameSyncStatus::Failed(_) => report.summary.errors += 1,
        }
        report.games.push(GameSyncOutcome {
            game: game_name_plain,
            status,
        });
    }

    // Print summary
    ui::report_summary(&report.summary);

    Ok(report)
}

fn sync_status(result: Result<()>, completed: GameSyncStatus) -> GameSyncStatus {
    match result {
        Ok(()) => completed,
        Err(e) => GameSyncStatus::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_for_returns_only_failed_game_errors() {
        let report = SyncReport {
            summary: SyncSummary {
                backed_up: 1,
                restored: 0,
                skipped: 1,
                errors: 1,
            },
            games: vec![
                GameSyncOutcome {
                    game: "Fine Game".to_string(),
                    status: GameSyncStatus::BackedUp,
                },
                GameSyncOutcome {
                    game: "Skipped Game".to_string(),
                    status: GameSyncStatus::Skipped,
                },
                GameSyncOutcome {
                    game: "Broken Game".to_string(),
                    status: GameSyncStatus::Failed("Save path does not exist".to_string()),
                },
            ],
        };

        assert_eq!(
            report.failure_for("Broken Game"),
            Some("Save path does not exist")
        );
        assert_eq!(report.failure_for("Fine Game"), None);
        assert_eq!(report.failure_for("Skipped Game"), None);
        assert_eq!(report.failure_for("Unknown Game"), None);
    }

    #[test]
    fn sync_status_maps_results() {
        assert_eq!(
            sync_status(Ok(()), GameSyncStatus::BackedUp),
            GameSyncStatus::BackedUp
        );
        assert_eq!(
            sync_status(Ok(()), GameSyncStatus::Restored),
            GameSyncStatus::Restored
        );
        assert_eq!(
            sync_status(Err(anyhow::anyhow!("restic failed")), GameSyncStatus::BackedUp),
            GameSyncStatus::Failed("restic failed".to_string())
        );
    }

    #[test]
    fn summary_counts_backups_and_restores_separately() {
        let summary = SyncSummary {
            backed_up: 2,
            restored: 1,
            skipped: 3,
            errors: 1,
        };

        assert_eq!(summary.total(), 7);
    }
}
