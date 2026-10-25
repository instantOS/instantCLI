use anyhow::{Context, Result, ensure};
use serde_json::json;

use super::ui;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::ownership::{OwnerId, SaveOwner};
use crate::game::restic::backup::{GameBackup, RestoreRequest};
use crate::game::restic::cache::SnapshotView;
use crate::game::save_runtime;
use crate::ui::Level;

pub fn perform_backup(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    content: &str,
    view: &mut SnapshotView,
) -> Result<()> {
    backup_with_intent(owner, config, false, Some(content), view)
}

pub fn publish_local_choice(owner: &SaveOwner, config: &InstantGameConfig) -> Result<()> {
    publish_local_choice_with_view(owner, config, &mut SnapshotView::default())
}

fn publish_local_choice_with_view(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    view: &mut SnapshotView,
) -> Result<()> {
    backup_with_intent(owner, config, true, None, view)
}

fn backup_with_intent(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    chosen: bool,
    content: Option<&str>,
    view: &mut SnapshotView,
) -> Result<()> {
    let snapshots = view.for_owner(config, &owner.id)?;
    let parents = save_runtime::history_heads(&snapshots)?;
    let expected = owner
        .baseline
        .as_ref()
        .and_then(|baseline| baseline.remote_head.as_deref());
    if !chosen && (parents.len() > 1 || parents.first().map(|head| head.id.as_str()) != expected) {
        // The observed head must match our baseline. An upload racing this
        // observation creates a branch, detected by the post-upload refresh.
        return Err(save_runtime::ConcurrentSaveVersions.into());
    }
    let mut tags = save_runtime::parent_tags(&parents);
    let alternatives = &parents;
    if alternatives.len() > 1
        || alternatives.first().is_some_and(|head| {
            owner
                .baseline
                .as_ref()
                .and_then(|baseline| baseline.remote_head.as_deref())
                != Some(head.id.as_str())
        })
    {
        tags.extend(
            alternatives
                .iter()
                .map(|snapshot| format!("instantgame-preserve:{}", snapshot.id)),
        );
    }
    view.invalidate();
    let result = ui::stage(format!("{}: Backing up saves...", owner.name), || {
        let result = GameBackup::new(config.clone())
            .backup_owner_with_content(owner, &tags, content)
            .context("Failed to back up saves")?;
        view.refresh(config)?;
        Ok(result)
    })?;
    let snapshots = view.for_owner(config, &owner.id)?;
    let id = result
        .snapshot_id
        .context("Backup produced no resolvable snapshot; reconcile before playing")?;
    let completed_id = id.clone();
    InstallationsConfig::update_owner(&owner.id, |saves| {
        saves.note_backup(&id);
        saves.sync_repository = Some(config.repo.as_path().to_string_lossy().into_owned());
        saves.baseline = Some(crate::game::config::SaveBaseline {
            content: result.content_digest,
            remote_head: Some(id),
        });
        Ok(())
    })?;
    if save_runtime::history_heads(&snapshots)?.len() > 1 {
        return Err(save_runtime::ConcurrentSaveVersions.into());
    }
    let message = if chosen {
        format!(
            "{}: Backed up chosen saves; snapshot: {completed_id}",
            owner.name
        )
    } else {
        format!("{}: Backed up saves; snapshot: {completed_id}", owner.name)
    };
    ui::report(
        Level::Success,
        "game.sync.backup.completed",
        &message,
        json!({"game": owner.name, "snapshot_id": completed_id, "action": "backup_completed"}),
    );
    Ok(())
}

pub fn perform_restore(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    snapshot_id: &str,
) -> Result<()> {
    perform_restore_with_view(owner, config, snapshot_id, &mut SnapshotView::default())
}

pub(crate) fn perform_restore_with_view(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    snapshot_id: &str,
    view: &mut SnapshotView,
) -> Result<()> {
    restore_selected(owner, config, snapshot_id, view, true)
}

fn restore_selected(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    snapshot_id: &str,
    view: &mut SnapshotView,
    report_completion: bool,
) -> Result<()> {
    let mut snapshots = view.for_owner(config, &owner.id)?;
    let selected = snapshots
        .iter()
        .find(|snapshot| snapshot.matches_id(snapshot_id))
        .context("Snapshot is not in this owner's history")?
        .clone();
    ensure!(
        selected.paths.len() == 1,
        "Snapshot must contain exactly one owned file or directory; explicit layout conversion is required"
    );
    if owner.pending_restore.is_none() {
        let digest = ui::stage(
            format!("{}: Checking local saves before restore...", owner.name),
            || save_runtime::content_digest(owner.save_path.as_path()),
        )?;
        let changed = owner
            .baseline
            .as_ref()
            .is_none_or(|baseline| baseline.content != digest);
        if changed
            && (owner.baseline.is_some() || save_runtime::has_save_data(owner.save_path.as_path())?)
            && digest != "missing"
            && save_runtime::snapshot_digest(&selected) != Some(digest.as_str())
        {
            // Preserve unuploaded local edits before replacing any shared data.
            // Its parent is the last acknowledged head, not the conflicting head.
            let parents: Vec<_> = owner
                .baseline
                .as_ref()
                .and_then(|baseline| baseline.remote_head.as_ref())
                .and_then(|id| snapshots.iter().find(|snapshot| &snapshot.id == id))
                .cloned()
                .into_iter()
                .collect();
            let mut tags = save_runtime::parent_tags(&parents);
            tags.push("instantgame-preserved".into());
            view.invalidate();
            ui::stage(
                format!("{}: Preserving local saves before restore...", owner.name),
                || {
                    GameBackup::new(config.clone()).backup_owner_with_content(
                        owner,
                        &tags,
                        Some(&digest),
                    )?;
                    view.refresh(config)
                },
            )?;
            snapshots = view.for_owner(config, &owner.id)?;
            ui::report(
                Level::Info,
                "game.sync.local_preserved",
                &format!(
                    "{}: Backed up local saves before replacing them",
                    owner.name
                ),
                json!({"game": owner.name}),
            );
        }
    }
    let heads = save_runtime::history_heads(&snapshots)?;
    let head = if heads.len() == 1 {
        heads.first().map(|snapshot| snapshot.id.as_str())
    } else {
        None
    };
    let snapshot = &selected;
    if matches!(owner.id, OwnerId::Group(_)) {
        ui::report(
            Level::Info,
            "game.sync.scope",
            &owner.scope(),
            json!({"game": owner.name, "members": owner.members}),
        );
    }
    ui::stage(
        format!(
            "{}: Restoring saves from {}...",
            owner.name, snapshot.hostname
        ),
        || {
            GameBackup::new(config.clone()).restore_backup(RestoreRequest {
                owner: &owner.id,
                snapshot_id: &snapshot.id,
                snapshot_time: &snapshot.time,
                path: owner.save_path.as_path(),
                save_path_type: owner.save_path_type,
                snapshot_source_path: snapshot.paths.first().map(String::as_str),
                observed_head: head,
            })
        },
    )?;
    if heads.len() > 1 {
        // Publish the explicitly chosen contents as a child of every branch.
        // All conflicting versions remain in history for recovery.
        publish_local_choice_with_view(owner, config, view)?;
    }
    if report_completion {
        ui::report(
            Level::Success,
            "game.sync.restore.completed",
            &format!("{}: Restored saves from {}", owner.name, snapshot.hostname),
            json!({"game": owner.name, "snapshot_id": snapshot.id, "hostname": snapshot.hostname, "action": "restore_completed"}),
        );
    }
    Ok(())
}

/// Caller holds the owner lock. Preserve candidates before applying the newest
/// contents, then publish the choice as a shared descendant of every branch.
pub fn resolve_newest(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    view: &mut SnapshotView,
) -> Result<(bool, String)> {
    let snapshots = view.for_owner(config, &owner.id)?;
    let heads = save_runtime::history_heads(&snapshots)?;
    let newest = heads
        .iter()
        .map(|snapshot| Ok((save_runtime::snapshot_save_time(snapshot)?, snapshot)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max_by(|(a_time, a), (b_time, b)| a_time.cmp(b_time).then_with(|| a.id.cmp(&b.id)));
    let local_time =
        if owner.baseline.is_some() || save_runtime::has_save_data(owner.save_path.as_path())? {
            save_runtime::latest_save_time(owner.save_path.as_path())?
        } else {
            None
        };
    let use_remote = match (&newest, local_time) {
        (Some((remote_time, _)), Some(local_time)) => *remote_time >= local_time,
        (Some(_), None) => true,
        (None, _) => false,
    };
    if use_remote {
        let (_, snapshot) = newest.context("No remote candidate")?;
        // Competing history tips may contain identical bytes. Join their
        // histories without restoring or uploading a redundant recovery copy.
        if let Some(remote) = save_runtime::snapshot_digest(snapshot)
            && remote
                == ui::stage(format!("{}: Comparing newest saves...", owner.name), || {
                    save_runtime::content_digest(owner.save_path.as_path())
                })?
        {
            publish_local_choice_with_view(owner, config, view)?;
            return Ok((
                false,
                format!(
                    "{}: Kept matching local saves and reconciled history; other versions retained in history",
                    owner.name
                ),
            ));
        }
        restore_selected(owner, config, &snapshot.id, view, false)?;
        Ok((
            true,
            format!(
                "{}: Restored newest saves from {} ({}); other versions retained in history",
                owner.name,
                snapshot.hostname,
                save_runtime::snapshot_save_time(snapshot)?.format("%Y-%m-%d %H:%M UTC")
            ),
        ))
    } else {
        ensure!(
            local_time.is_some(),
            "No local saves or remote snapshots are available for {}",
            owner.name
        );
        publish_local_choice_with_view(owner, config, view)?;
        Ok((
            false,
            format!(
                "{}: Backed up this device's newest saves; other versions retained in history",
                owner.name
            ),
        ))
    }
}

pub fn acknowledge_snapshot(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    id: &str,
    expected_content: &str,
) -> Result<()> {
    let content = save_runtime::content_digest(owner.save_path.as_path())?;
    ensure!(
        content == expected_content,
        "Saves changed while checking sync status; retry syncing"
    );
    InstallationsConfig::update_owner(&owner.id, |saves| {
        saves.note_backup(id);
        saves.sync_repository = Some(config.repo.as_path().to_string_lossy().into_owned());
        saves.baseline = Some(crate::game::config::SaveBaseline {
            content,
            remote_head: Some(id.into()),
        });
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::game::config::{OwnedSaves, PathContentKind};

    #[test]
    fn changed_saves_cannot_be_acknowledged_as_matching_remote_contents() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("save");
        std::fs::write(&path, b"before")?;
        let observed = save_runtime::content_digest(&path)?;
        std::fs::write(&path, b"after!")?;
        let owner = SaveOwner {
            id: OwnerId::Game("Test".into()),
            name: "Test".into(),
            members: vec!["Test".into()],
            saves: OwnedSaves::new(
                TildePath::from_str(&path.to_string_lossy()),
                PathContentKind::File,
            ),
        };
        let result = acknowledge_snapshot(&owner, &InstantGameConfig::default(), "head", &observed);
        assert!(result.is_err_and(|error| error.to_string().contains("Saves changed")));
        Ok(())
    }
}
