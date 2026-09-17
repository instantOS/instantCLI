//! Explicitly reconcile local saves with a repository, including repository switches.
use anyhow::{Context, Result, bail};

use crate::game::config::{GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::game::restic::backup::{GameBackup, RestoreRequest};
use crate::game::restic::cache;
use crate::game::restic::snapshot_selection::{EnhancedSnapshot, SnapshotMenuEntry};
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::game::utils::save_files::{format_system_time_for_display, get_save_directory_info};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::restic::wrapper::Snapshot;
use crate::ui::nerd_font::NerdFont;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SaveChoice {
    Upload,
    Restore(String),
    Reselect,
    Cancel,
}

#[derive(Clone)]
struct Choice {
    label: &'static str,
    description: &'static str,
    action: MenuAction,
}

#[derive(Clone)]
enum MenuAction {
    Upload,
    Latest,
    History,
    Reselect,
}

impl FzfSelectable for Choice {
    fn fzf_display_text(&self) -> String {
        self.label.into()
    }
    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(self.description.into())
    }
}

pub(crate) fn choose_saves(
    installation: &GameInstallation,
    snapshots: &[Snapshot],
) -> Result<SaveChoice> {
    let local = get_save_directory_info(installation.save_path.as_path())?;
    let latest = snapshots.first();
    let backup_info = latest
        .map(|snapshot| {
            format!(
                "Latest backup created: {}\nDevice: {}\nSnapshot: {}",
                snapshot.time, snapshot.hostname, snapshot.id
            )
        })
        .unwrap_or_else(|| "No backups in this repository.".into());
    let summary = format!(
        "Game: {}\nLocal path: {}\nLocal files last modified: {}\nLocal contents: {} files, {} bytes\n{}\nModification time and backup creation time are different measurements, not proof of game progress.\nRestoring replaces local saves, including removing files absent from the backup. No recovery copy is kept.",
        installation.game_name.0,
        installation.save_path.display_string(),
        format_system_time_for_display(local.last_modified),
        local.file_count,
        local.total_size,
        backup_info
    );
    loop {
        let mut choices = Vec::new();
        if local.file_count > 0 {
            choices.push(Choice {
                label: "Use local saves — upload now",
                description: "Back up local saves and use them on this device. This becomes the newest backup and may be downloaded by other devices.",
                action: MenuAction::Upload,
            });
        }
        if latest.is_some() {
            choices.push(Choice {
                label: "Use latest backup — restore locally",
                description: "Replace local saves with the latest backup. Local changes that were never backed up will be lost.",
                action: MenuAction::Latest,
            });
            choices.push(Choice {
                label: "Choose a backup to restore…",
                description: "Compare backup dates and devices, then choose a historical version. This device keeps that choice until local changes or a new remote backup appear.",
                action: MenuAction::History,
            });
        }
        choices.push(Choice {
            label: "Choose a different save path…",
            description: "Return to local save path selection without uploading or restoring.",
            action: MenuAction::Reselect,
        });
        let action = match FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::BackupRestore, "Choose which saves to use")
                    .subtitle(&summary),
            )
            .items(choices)
            .padded()
            .select_one()?
        {
            DialogOutcome::Submitted(choice) => choice.action,
            DialogOutcome::Cancelled => return Ok(SaveChoice::Cancel),
        };
        match action {
            MenuAction::Upload => return Ok(SaveChoice::Upload),
            MenuAction::Latest => {
                return Ok(SaveChoice::Restore(
                    latest.context("No latest snapshot")?.id.clone(),
                ));
            }
            MenuAction::Reselect => return Ok(SaveChoice::Reselect),
            MenuAction::History => {
                let mut entries: Vec<_> = snapshots
                    .iter()
                    .cloned()
                    .map(|snapshot| {
                        SnapshotMenuEntry::Snapshot(EnhancedSnapshot {
                            snapshot,
                            local_save_info: Some(local.clone()),
                            game_name: installation.game_name.0.clone(),
                            nearest_checkpoint: installation.nearest_checkpoint.clone(),
                        })
                    })
                    .collect();
                entries.push(SnapshotMenuEntry::Back);
                if let DialogOutcome::Submitted(SnapshotMenuEntry::Snapshot(selected)) =
                    FzfWrapper::builder()
                        .header("Select backup to restore (replaces local saves)")
                        .items(entries)
                        .select_one()?
                {
                    return Ok(SaveChoice::Restore(selected.snapshot.id));
                }
            }
        }
    }
}

/// Save the retry marker before touching live files. Clear it only after success.
pub(crate) fn apply_choice(
    config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
    index: usize,
    choice: SaveChoice,
) -> Result<()> {
    let repository = config.repo.as_path().to_string_lossy().to_string();
    let installation = &installations.installations[index];
    ensure_safe_path(installation.save_path.as_path(), PathUsage::SaveDirectory)?;
    let game_name = installation.game_name.0.clone();
    let handler = GameBackup::new(config.clone());
    match choice {
        SaveChoice::Upload => {
            let result = handler.backup_game(installation)?;
            cache::invalidate_snapshot_cache();
            let id = crate::game::checkpoint::resolve_backup_snapshot_id(
                result.snapshot_id.as_deref(),
                &game_name,
                config,
            )?
            .context("Backup completed but no checkpoint could be resolved; run setup again")?;
            installations.installations[index].update_checkpoint(id);
        }
        SaveChoice::Restore(id) => {
            let snapshots = cache::get_snapshots_for_game(&game_name, config)?;
            let snapshot = snapshots
                .iter()
                .find(|snapshot| snapshot.matches_id(&id))
                .context("Selected backup is no longer available")?;
            let head = snapshots.first().map(|snapshot| snapshot.id.clone());
            // Persist any reselected path or newly-created installation before
            // the managed restore writes its durable retry marker.
            installations
                .save()
                .context("Could not save installation before restore; no files were restored")?;
            let installation = &installations.installations[index];
            handler
                .restore_backup(RestoreRequest {
                    game_name: &game_name,
                    snapshot_id: &snapshot.id,
                    path: installation.save_path.as_path(),
                    save_path_type: installation.save_path_type,
                    snapshot_source_path: snapshot.paths.first().map(String::as_str),
                })
                .context("Could not reconcile saves from the selected backup")?;
            *installations = InstallationsConfig::load()
                .context("Could not reload installation after restore")?;
            let installation = &mut installations.installations[index];
            // The checkpoint describes actual contents; the acknowledged head only
            // prevents an immediate re-restore of a newer, deliberately rejected version.
            installation.acknowledged_snapshot = head;
        }
        SaveChoice::Cancel | SaveChoice::Reselect => bail!("No save action selected"),
    }
    installations.installations[index].sync_repository = Some(repository);
    installations
        .save()
        .context("Could not save completed save choice; run setup again before playing")?;
    cache::invalidate_snapshot_cache();
    Ok(())
}

/// Pending operations and repository changes must be resolved before automatic sync.
pub(crate) fn reconcile_configured_games(config: &InstantGameConfig) -> Result<bool> {
    let mut installations = InstallationsConfig::load()?;
    let repository = config.repo.as_path().to_string_lossy().to_string();
    for index in 0..installations.installations.len() {
        let installation = &installations.installations[index];
        if let Some(id) = installation.pending_restore.clone() {
            if FzfWrapper::confirm(&format!(
                "Restore for '{}' was incomplete. Retry the selected backup now?",
                installation.game_name.0
            ))? != crate::menu_utils::ConfirmResult::Yes
            {
                return Ok(false);
            }
            apply_choice(config, &mut installations, index, SaveChoice::Restore(id))?;
            continue;
        }
        if !installation.needs_repository_reconciliation(&repository) {
            continue;
        }
        loop {
            let installation = &installations.installations[index];
            let snapshots = cache::get_snapshots_for_game(&installation.game_name.0, config)?;
            match choose_saves(installation, &snapshots)? {
                SaveChoice::Cancel => return Ok(false),
                SaveChoice::Reselect => {
                    let selected =
                        crate::game::setup::choose_reconciliation_path(&installation.game_name.0)?;
                    let Some((path, kind)) = selected else {
                        continue;
                    };
                    installations.installations[index].save_path = path;
                    installations.installations[index].save_path_type = kind;
                }
                choice => {
                    apply_choice(config, &mut installations, index, choice)?;
                    break;
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::game::config::PathContentKind;
    use crate::menu_utils::MockQueue;

    #[test]
    fn local_choice_is_upload_not_skip() -> Result<()> {
        let temp = tempfile::tempdir()?;
        // get_save_directory_info filters hidden entries (tempfile's root is
        // `.tmp…`), so the save directory must be a visible subdirectory.
        let saves = temp.path().join("saves");
        std::fs::create_dir(&saves)?;
        std::fs::write(saves.join("save"), "progress")?;
        let installation =
            GameInstallation::with_kind("test", TildePath::new(saves), PathContentKind::Directory);
        let _guard = MockQueue::new().select_index(0).guard();
        assert_eq!(choose_saves(&installation, &[])?, SaveChoice::Upload);
        Ok(())
    }

    #[test]
    fn escape_does_not_choose_local_or_restore() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let installation = GameInstallation::with_kind(
            "test",
            TildePath::new(temp.path().into()),
            PathContentKind::Directory,
        );
        let _guard = MockQueue::new().cancel_selection().guard();
        assert_eq!(choose_saves(&installation, &[])?, SaveChoice::Cancel);
        Ok(())
    }
}
