//! Explicitly reconcile local saves with a repository, including repository switches.
use anyhow::{Context, Result, bail};

use crate::game::config::{GameInstallation, InstallationsConfig, InstantGameConfig};
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
    choose_owned_saves(
        &installation.game_name.0,
        installation.owned()?,
        snapshots,
        "",
    )
}

fn choose_owned_saves(
    game_name: &str,
    installation: &crate::game::config::OwnedSaves,
    snapshots: &[Snapshot],
    scope: &str,
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
    let recovery_warning = installation.pending_restore.as_ref().map_or("", |_| {
        "\nWARNING: The previous restore did not complete. Current local files may be a partial restore; upload them only if you deliberately want to keep that state."
    });
    let summary = format!(
        "Game: {}\nLocal path: {}\nLocal files last modified: {}\nLocal contents: {} files, {} bytes\n{}{}\nModification time and backup creation time are different measurements, not proof of game progress.\nRestoring replaces local saves, including removing files absent from the backup. No recovery copy is kept.",
        game_name,
        installation.save_path.display_string(),
        format_system_time_for_display(local.last_modified),
        local.file_count,
        local.total_size,
        backup_info,
        recovery_warning
    );
    loop {
        let mut choices = Vec::new();
        if local.file_count > 0 {
            choices.push(Choice {
                label: if installation.pending_restore.is_some() {
                    "Accept current local files — upload now"
                } else {
                    "Use local saves — upload now"
                },
                description: if installation.pending_restore.is_some() {
                    "Explicitly accept the possibly partial local restore, back it up, and clear the incomplete-restore state."
                } else {
                    "Back up local saves and use them on this device. This becomes the newest backup and may be downloaded by other devices."
                },
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
                    .subtitle(format!("{scope}\n{summary}")),
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
                            game_name: game_name.into(),
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

/// Private setup adapter: persist the binding, then use the same owner operation.
pub(crate) fn apply_choice(
    config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
    index: usize,
    choice: SaveChoice,
) -> Result<()> {
    config.validate_ownership(installations)?;
    installations.save()?;
    let id = config.owner_id(&installations.installations[index].game_name.0);
    let _lock = crate::game::save_runtime::SaveLock::owner(&id)?;
    let owner = config.resolve_owner(installations, &id)?;
    apply_owner_choice(config, &owner, choice)?;
    *installations = InstallationsConfig::load()?;
    Ok(())
}

pub(crate) fn apply_owner_choice(
    config: &InstantGameConfig,
    owner: &crate::game::ownership::SaveOwner,
    choice: SaveChoice,
) -> Result<()> {
    ensure_safe_path(owner.save_path.as_path(), PathUsage::SaveDirectory)?;
    match choice {
        SaveChoice::Upload => {
            crate::game::operations::sync::execution::publish_local_choice(owner, config)
        }
        SaveChoice::Restore(id) => {
            crate::game::operations::sync::execution::perform_restore(owner, config, &id)
        }
        SaveChoice::Cancel | SaveChoice::Reselect => bail!("No save action selected"),
    }
}

pub(crate) fn reconcile_owner(
    config: &InstantGameConfig,
    id: &crate::game::ownership::OwnerId,
) -> Result<bool> {
    let _lock = crate::game::save_runtime::SaveLock::owner(id)?;
    let local = InstallationsConfig::load()?;
    config.validate_ownership(&local)?;
    let owner = config.resolve_owner(&local, id)?;
    let snapshots = crate::game::save_runtime::fresh_snapshots(config, id)?;
    println!("{}", owner.scope());
    match choose_owned_saves(&owner.name, &owner.saves, &snapshots, &owner.scope())? {
        SaveChoice::Cancel => Ok(false),
        SaveChoice::Reselect => bail!(
            "Change this owner's save path with relocate (private saves) or group bind (shared saves), then reconcile again"
        ),
        choice => {
            apply_owner_choice(config, &owner, choice)?;
            Ok(true)
        }
    }
}

pub(crate) fn reconcile_configured_games(config: &InstantGameConfig) -> Result<bool> {
    let local = InstallationsConfig::load()?;
    config.validate_ownership(&local)?;
    let repository = config.repo.as_path().to_string_lossy().into_owned();
    for id in config.bound_owner_ids(&local) {
        let saves = local.owner_saves(&id)?;
        if (saves.pending_restore.is_some()
            || saves.needs_repository_reconciliation(&repository)
            || saves.baseline.is_none())
            && !reconcile_owner(config, &id)?
        {
            return Ok(false);
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
