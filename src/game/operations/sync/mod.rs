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
    /// Reject any failed game, including failures recorded only in the summary.
    pub fn ensure_success(&self) -> Result<()> {
        let failures: Vec<String> = self
            .games
            .iter()
            .filter_map(|outcome| match &outcome.status {
                GameSyncStatus::Failed(message) => Some(format!("{}: {}", outcome.game, message)),
                _ => None,
            })
            .collect();
        let failed = self.summary.errors.max(failures.len());
        if failed == 0 {
            return Ok(());
        }

        let mut message = format!("Save sync failed: {failed} failed game(s).");
        if !failures.is_empty() {
            message.push('\n');
            message.push_str(&failures.join("\n"));
        }
        if self.summary.errors > failures.len() {
            message.push_str(&format!(
                "\n{} failure(s) reported without per-game details.",
                self.summary.errors - failures.len()
            ));
        }
        Err(anyhow::anyhow!(message))
    }

    /// Describe actual sync results without implying every run created a backup.
    pub fn completion_message(&self) -> String {
        if self.summary.total() == 0 && self.games.is_empty() {
            return "No games configured for syncing.".to_string();
        }

        format!(
            "Save sync results: {} backed up, {} restored, {} skipped, {} failed.",
            self.summary.backed_up,
            self.summary.restored,
            self.summary.skipped,
            self.summary.errors
        )
    }

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
///
/// The progress callback receives each stage before its terminal spinner starts.
/// Callers that do not need progress updates can pass `&mut |_| {}`.
pub fn sync_game_saves(
    game_name: Option<String>,
    force: bool,
    progress: &mut dyn FnMut(&str),
) -> Result<SyncReport> {
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

        let message = format!("{}: Checking sync status...", game_name_plain);
        progress(&message);
        let spinner = create_spinner(message);
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
                    let message = format!("{}: Creating backup...", game_name_plain);
                    progress(&message);
                    let spinner = create_spinner(message);
                    let result = execution::perform_backup(&installation, &game_config);
                    spinner.finish_and_clear();
                    ui::report_backup_result(&game_name_plain, &result);
                    sync_status(result, GameSyncStatus::BackedUp)
                }
                SyncAction::RestoreFromSnapshot(snapshot_id) => {
                    let message = format!("{}: Restoring from snapshot...", game_name_plain);
                    progress(&message);
                    let spinner = create_spinner(message);
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    spinner.finish_and_clear();
                    ui::report_restore_result(&game_name_plain, &snapshot_id, &result);
                    sync_status(result, GameSyncStatus::Restored)
                }
                SyncAction::RestoreFromLatest(snapshot_id) => {
                    let message = format!("{}: Restoring latest backup...", game_name_plain);
                    progress(&message);
                    let spinner = create_spinner(message);
                    let result =
                        execution::perform_restore(&installation, &game_config, &snapshot_id);
                    spinner.finish_and_clear();
                    ui::report_restore_latest_result(&game_name_plain, &snapshot_id, &result);
                    sync_status(result, GameSyncStatus::Restored)
                }
                SyncAction::CreateInitialBackup => {
                    let message = format!("{}: Creating initial backup...", game_name_plain);
                    progress(&message);
                    let spinner = create_spinner(message);
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
        assert!(report.ensure_success().is_err());
        assert_eq!(
            report.completion_message(),
            "Save sync results: 1 backed up, 0 restored, 1 skipped, 1 failed."
        );
    }

    #[test]
    fn ensure_success_rejects_individual_failures_with_game_details() {
        let mut report = SyncReport {
            summary: SyncSummary {
                errors: 2,
                ..SyncSummary::default()
            },
            games: vec![
                GameSyncOutcome {
                    game: "Broken Game".to_string(),
                    status: GameSyncStatus::Failed("Save path does not exist".to_string()),
                },
                GameSyncOutcome {
                    game: "Other Game".to_string(),
                    status: GameSyncStatus::Failed("Repository unavailable".to_string()),
                },
            ],
        };

        for summary_errors in [2, 0] {
            report.summary.errors = summary_errors;
            let error = report.ensure_success().unwrap_err().to_string();
            assert!(error.contains("2 failed game(s)"));
            assert!(error.contains("Broken Game: Save path does not exist"));
            assert!(error.contains("Other Game: Repository unavailable"));
        }
    }

    #[test]
    fn ensure_success_rejects_summary_errors_without_game_details() {
        let report = SyncReport {
            summary: SyncSummary {
                errors: 3,
                ..SyncSummary::default()
            },
            games: Vec::new(),
        };

        let error = report.ensure_success().unwrap_err().to_string();
        assert!(error.contains("3 failed game(s)"));
        assert!(error.contains("3 failure(s) reported without per-game details"));
        assert_eq!(
            report.completion_message(),
            "Save sync results: 0 backed up, 0 restored, 0 skipped, 3 failed."
        );
    }

    #[test]
    fn successful_report_reports_backups_and_restores_honestly() {
        let report = SyncReport {
            summary: SyncSummary {
                backed_up: 1,
                restored: 1,
                ..SyncSummary::default()
            },
            games: vec![
                GameSyncOutcome {
                    game: "Backed Up Game".to_string(),
                    status: GameSyncStatus::BackedUp,
                },
                GameSyncOutcome {
                    game: "Restored Game".to_string(),
                    status: GameSyncStatus::Restored,
                },
            ],
        };

        assert!(report.ensure_success().is_ok());
        assert_eq!(
            report.completion_message(),
            "Save sync results: 1 backed up, 1 restored, 0 skipped, 0 failed."
        );
    }

    #[test]
    fn skipped_report_does_not_claim_a_backup() {
        let report = SyncReport {
            summary: SyncSummary {
                skipped: 1,
                ..SyncSummary::default()
            },
            games: vec![GameSyncOutcome {
                game: "Already Synced Game".to_string(),
                status: GameSyncStatus::Skipped,
            }],
        };

        assert!(report.ensure_success().is_ok());
        assert_eq!(
            report.completion_message(),
            "Save sync results: 0 backed up, 0 restored, 1 skipped, 0 failed."
        );
    }

    #[test]
    fn empty_report_reports_no_games_configured() {
        let report = SyncReport::default();

        assert!(report.ensure_success().is_ok());
        assert_eq!(
            report.completion_message(),
            "No games configured for syncing."
        );
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
            sync_status(
                Err(anyhow::anyhow!("restic failed")),
                GameSyncStatus::BackedUp
            ),
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
