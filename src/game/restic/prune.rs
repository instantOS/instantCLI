use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Datelike, Utc};

use crate::game::config::{InstallationsConfig, InstantGameConfig, RetentionPolicyValues};
use crate::game::ownership::OwnerId;
use crate::game::save_runtime::{self, SaveLock};
use crate::restic::wrapper::{ResticWrapper, Snapshot};

pub fn prune_snapshots(game_name: Option<String>, zero_changes: bool) -> Result<()> {
    let config = InstantGameConfig::load()?;
    let local = InstallationsConfig::load()?;
    config.validate_ownership(&local)?;
    let mut ids = if let Some(name) = game_name {
        vec![config.owner_id(&name)]
    } else {
        let mut ids: Vec<_> = config
            .games
            .iter()
            .map(|game| config.owner_id(&game.name.0))
            .collect();
        ids.extend(
            config
                .save_groups
                .iter()
                .map(|group| OwnerId::Group(group.id.clone())),
        );
        ids
    };
    ids.sort();
    ids.dedup();
    let restic = ResticWrapper::new(
        config.repo.as_path().to_string_lossy().into_owned(),
        config.repo_password.clone(),
    )?;
    if !zero_changes {
        println!(
            "Retention policy per save owner: {:?}",
            config.retention_policy.effective().to_rules()
        );
    }
    for id in ids {
        let _lock = SaveLock::owner(&id)?;
        let current = InstallationsConfig::load()?;
        let snapshots = save_runtime::fresh_snapshots(&config, &id)?;
        let shared = matches!(id, OwnerId::Group(_));
        let heads = save_runtime::history_heads(&snapshots)?;
        ensure!(
            heads.len() <= 1,
            "{} has unresolved branches; pruning is blocked until reconciliation",
            id.label()
        );
        let mut keep: HashSet<_> = heads.iter().map(|snapshot| snapshot.id.clone()).collect();
        if let Ok(saves) = current.owner_saves(&id) {
            ensure!(
                saves.pending_restore.is_none(),
                "{} has an incomplete restore; pruning is blocked",
                id.label()
            );
            if shared {
                let digest = save_runtime::content_digest(saves.save_path.as_path())?;
                ensure!(
                    !matches!(
                        save_runtime::baseline_action(
                            saves,
                            &digest,
                            heads.first().map(|head| head.id.as_str())
                        ),
                        save_runtime::BaselineAction::Reconcile
                    ),
                    "{} needs reconciliation; pruning is blocked",
                    id.label()
                );
            }
            for pin in [
                &saves.nearest_checkpoint,
                &saves.acknowledged_snapshot,
                &saves
                    .baseline
                    .as_ref()
                    .and_then(|baseline| baseline.remote_head.clone()),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(snapshot) = snapshots.iter().find(|snapshot| snapshot.matches_id(pin)) {
                    keep.insert(snapshot.id.clone());
                }
            }
        }
        for snapshot in &snapshots {
            if snapshot
                .tags
                .iter()
                .any(|tag| tag == "instantgame-preserved")
            {
                keep.insert(snapshot.id.clone());
            }
            for preserved in snapshot
                .tags
                .iter()
                .filter_map(|tag| tag.strip_prefix("instantgame-preserve:"))
            {
                keep.insert(preserved.into());
            }
        }
        if zero_changes {
            let mut retained_zero_change = false;
            for snapshot in &snapshots {
                let zero = snapshot
                    .summary
                    .as_ref()
                    .is_some_and(|summary| summary.files_new == 0 && summary.files_changed == 0);
                if !zero || !retained_zero_change {
                    keep.insert(snapshot.id.clone());
                }
                retained_zero_change |= zero;
            }
        } else {
            keep.extend(retained_ids(
                &snapshots,
                config.retention_policy.effective(),
            )?);
        }
        // Retain the latest legacy anchor until all old private snapshots are
        // explicitly removed, so its implicit linear history stays unambiguous.
        if let Some(legacy) = snapshots.iter().find(|snapshot| {
            !snapshot
                .tags
                .iter()
                .any(|tag| tag.starts_with("instantgame-parent:"))
        }) {
            keep.insert(legacy.id.clone());
        }
        retain_descendants(&snapshots, &mut keep);
        let targets: Vec<_> = snapshots
            .iter()
            .filter(|snapshot| !keep.contains(&snapshot.id))
            .map(|snapshot| snapshot.id.clone())
            .collect();
        if !targets.is_empty() {
            restic
                .forget_snapshots(&targets, true)
                .context("Pruning save history")?;
        }
        println!(
            "{}: removed {} snapshots; active heads and recovery checkpoints retained",
            id.label(),
            targets.len()
        );
        super::cache::invalidate_snapshot_cache();
    }
    Ok(())
}

fn retained_ids(snapshots: &[Snapshot], policy: RetentionPolicyValues) -> Result<HashSet<String>> {
    let mut keep = HashSet::new();
    for snapshot in snapshots
        .iter()
        .take(policy.keep_last.unwrap_or(0) as usize)
    {
        keep.insert(snapshot.id.clone());
    }
    let mut daily = HashSet::new();
    let mut weekly = HashSet::new();
    let mut monthly = HashSet::new();
    let mut yearly = HashSet::new();
    for snapshot in snapshots {
        let time: DateTime<Utc> = DateTime::parse_from_rfc3339(&snapshot.time)?.with_timezone(&Utc);
        let mut selected = false;
        if daily.len() < policy.keep_daily as usize && daily.insert(time.date_naive()) {
            selected = true;
        }
        if weekly.len() < policy.keep_weekly as usize
            && weekly.insert((time.iso_week().year(), time.iso_week().week()))
        {
            selected = true;
        }
        if monthly.len() < policy.keep_monthly as usize
            && monthly.insert((time.year(), time.month()))
        {
            selected = true;
        }
        if yearly.len() < policy.keep_yearly as usize && yearly.insert(time.year()) {
            selected = true;
        }
        if selected {
            keep.insert(snapshot.id.clone());
        }
    }
    Ok(keep)
}

/// Keeping an old checkpoint also keeps the path from it to the current head.
/// Otherwise removing an intermediate node would make a retained ancestor look
/// like an independent branch. Missing ancestors below the retained suffix are safe.
fn retain_descendants(snapshots: &[Snapshot], keep: &mut HashSet<String>) {
    let mut children: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for snapshot in snapshots {
        for parent in snapshot
            .tags
            .iter()
            .filter_map(|tag| tag.strip_prefix("instantgame-parent:"))
        {
            children.entry(parent).or_default().push(&snapshot.id);
        }
    }
    let mut pending: Vec<_> = keep.iter().cloned().collect();
    while let Some(parent) = pending.pop() {
        if let Some(descendants) = children.get(parent.as_str()) {
            for child in descendants {
                if keep.insert((*child).into()) {
                    pending.push((*child).into());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: &str, parent: &str) -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "id": id, "short_id": id, "time": "2026-01-01T00:00:00Z", "tree": "tree", "paths": ["/saves"], "hostname": "device", "username": "user", "tags": [format!("instantgame-parent:{parent}")]
        })).unwrap()
    }

    #[test]
    fn pruning_preserves_the_path_between_a_pinned_checkpoint_and_head() -> Result<()> {
        let history = vec![
            snapshot("a", "root"),
            snapshot("b", "a"),
            snapshot("c", "b"),
            snapshot("d", "c"),
        ];
        let mut keep = HashSet::from(["b".into(), "d".into()]);
        retain_descendants(&history, &mut keep);
        assert_eq!(keep, HashSet::from(["b".into(), "c".into(), "d".into()]));
        let retained: Vec<_> = history
            .into_iter()
            .filter(|snapshot| keep.contains(&snapshot.id))
            .collect();
        assert_eq!(
            save_runtime::history_heads(&retained)?
                .iter()
                .map(|snapshot| snapshot.id.as_str())
                .collect::<Vec<_>>(),
            ["d"]
        );
        Ok(())
    }
}
