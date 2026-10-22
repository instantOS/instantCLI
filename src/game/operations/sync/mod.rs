mod decision;
pub(crate) mod execution;
mod types;

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

    pub fn conflict_messages(&self) -> Vec<&str> {
        let mut messages: Vec<_> = self
            .games
            .iter()
            .filter_map(|outcome| match &outcome.status {
                GameSyncStatus::BackedUpNewest(message)
                | GameSyncStatus::RestoredNewest(message) => Some(message.as_str()),
                _ => None,
            })
            .collect();
        messages.sort_unstable();
        messages.dedup();
        messages
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

    sync_configured(
        game_name,
        force,
        progress,
        game_config,
        installations,
        &[],
        true,
    )
}

pub(crate) fn sync_held_owner(
    game_name: String,
    owner: &crate::game::ownership::OwnerId,
    progress: &mut dyn FnMut(&str),
) -> Result<SyncReport> {
    let config = InstantGameConfig::load()?;
    anyhow::ensure!(
        config.owner_id(&game_name) == *owner,
        "Save ownership changed during play; synchronize the new configuration explicitly"
    );
    sync_configured(
        Some(game_name),
        false,
        progress,
        config,
        InstallationsConfig::load()?,
        std::slice::from_ref(owner),
        true,
    )
}

pub(crate) fn sync_held_all(
    owners: &[crate::game::ownership::OwnerId],
    progress: &mut dyn FnMut(&str),
) -> Result<SyncReport> {
    if owners.is_empty() {
        return Ok(SyncReport::default());
    }
    sync_configured(
        None,
        false,
        progress,
        InstantGameConfig::load()?,
        InstallationsConfig::load()?,
        owners,
        true,
    )
}

fn sync_configured(
    game_name: Option<String>,
    force: bool,
    progress: &mut dyn FnMut(&str),
    game_config: InstantGameConfig,
    installations: InstallationsConfig,
    held_owners: &[crate::game::ownership::OwnerId],
    use_disk: bool,
) -> Result<SyncReport> {
    game_config.validate_ownership(&installations)?;
    let owners_to_sync = match game_name {
        Some(name) => vec![game_config.owner_id(&name)],
        None if !held_owners.is_empty() => held_owners.to_vec(),
        None => game_config.bound_owner_ids(&installations),
    };

    if owners_to_sync.is_empty() {
        println!("No save owners configured for syncing.");
        return Ok(SyncReport::default());
    }

    // Empty installations need neither a repository nor restic.
    validation::check_restic_and_game_manager(&game_config)?;

    let mut report = SyncReport::default();
    let mut view = crate::game::restic::cache::SnapshotView::default();

    // Shared owners are visited once, with outcomes propagated to every member.
    for owner_id in owners_to_sync {
        let _owner_lock = if held_owners.contains(&owner_id) {
            None
        } else {
            Some(crate::game::save_runtime::SaveLock::owner(&owner_id)?)
        };
        // Reload after acquiring the lock: another operation may have advanced state.
        let current = if use_disk {
            InstallationsConfig::load()?
        } else {
            installations.clone()
        };
        game_config.validate_ownership(&current)?;
        let installation = game_config.resolve_owner(&current, &owner_id)?;
        let game_name_plain = installation.name.clone();

        let message = format!("{}: Checking sync status...", game_name_plain);
        progress(&message);
        let spinner = create_spinner(message);
        let action_result =
            decision::determine_action(&installation, &game_config, force, &mut view);
        spinner.finish_and_clear();
        let result = match action_result {
            Ok(action) => execute_action(&installation, &game_config, action, &mut view),
            Err(error) => Err(error),
        };
        let status = result.unwrap_or_else(|error| GameSyncStatus::Failed(format!("{error:#}")));
        if let GameSyncStatus::Failed(message) = &status {
            eprintln!("{}: {message}", installation.name);
        }

        match status {
            GameSyncStatus::BackedUp | GameSyncStatus::BackedUpNewest(_) => {
                report.summary.backed_up += 1
            }
            GameSyncStatus::Restored | GameSyncStatus::RestoredNewest(_) => {
                report.summary.restored += 1
            }
            GameSyncStatus::Skipped => report.summary.skipped += 1,
            GameSyncStatus::Failed(_) => report.summary.errors += 1,
        }
        for member in &installation.members {
            report.games.push(GameSyncOutcome {
                game: member.clone(),
                status: status.clone(),
            });
        }
    }

    // Print summary
    println!("{}", report.completion_message());

    Ok(report)
}

#[cfg(test)]
fn sync_status(result: Result<()>, completed: GameSyncStatus) -> GameSyncStatus {
    match result {
        Ok(()) => completed,
        Err(e) => GameSyncStatus::Failed(e.to_string()),
    }
}

pub(crate) fn sync_single_owner(
    owner: &crate::game::ownership::SaveOwner,
    config: &InstantGameConfig,
) -> Result<()> {
    let mut view = crate::game::restic::cache::SnapshotView::default();
    let status = execute_action(
        owner,
        config,
        decision::determine_action(owner, config, false, &mut view)?,
        &mut view,
    )?;
    if let GameSyncStatus::BackedUpNewest(message) | GameSyncStatus::RestoredNewest(message) =
        status
    {
        println!("{message}");
    }
    Ok(())
}

fn execute_action(
    owner: &crate::game::ownership::SaveOwner,
    config: &InstantGameConfig,
    mut action: SyncAction,
    view: &mut crate::game::restic::cache::SnapshotView,
) -> Result<GameSyncStatus> {
    let mut current = owner.clone();
    for attempt in 0..3 {
        match execute_once(&current, config, action, view) {
            Err(error)
                if error.is::<crate::game::save_runtime::ConcurrentSaveVersions>()
                    && attempt < 2 =>
            {
                current = config.resolve_owner(&InstallationsConfig::load()?, &owner.id)?;
                action = SyncAction::ResolveNewest;
            }
            result => return result,
        }
    }
    Err(crate::game::save_runtime::ConcurrentSaveVersions.into())
}

fn execute_once(
    owner: &crate::game::ownership::SaveOwner,
    config: &InstantGameConfig,
    action: SyncAction,
    view: &mut crate::game::restic::cache::SnapshotView,
) -> Result<GameSyncStatus> {
    match action {
        SyncAction::NoActionNeeded => Ok(GameSyncStatus::Skipped),
        SyncAction::CreateBackup => {
            execution::perform_backup(owner, config, view)?;
            Ok(GameSyncStatus::BackedUp)
        }
        SyncAction::RestoreFromSnapshot(id) => {
            execution::perform_restore_with_view(owner, config, &id, view)?;
            Ok(GameSyncStatus::Restored)
        }
        SyncAction::AcknowledgeSnapshot(id) => {
            execution::acknowledge_snapshot(owner, config, &id)?;
            Ok(GameSyncStatus::Skipped)
        }
        SyncAction::ResolveNewest => {
            let (restored, message) = execution::resolve_newest(owner, config, view)?;
            println!("{message}");
            Ok(if restored {
                GameSyncStatus::RestoredNewest(message)
            } else {
                GameSyncStatus::BackedUpNewest(message)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_installations_sync_without_initialized_repository() {
        let report = sync_configured(
            None,
            false,
            &mut |_| panic!("Empty sync must not report progress"),
            InstantGameConfig::default(),
            InstallationsConfig::default(),
            &[],
            false,
        )
        .unwrap();
        assert!(report.games.is_empty());
        assert_eq!(report.summary.errors, 0);
    }

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
