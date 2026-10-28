use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Datelike, Utc};

use crate::common::progress::create_spinner;
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
                    heads
                        .first()
                        .is_some_and(|head| save_runtime::snapshot_matches_baseline(saves, head))
                        || !matches!(
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
        let targets: Vec<_> = snapshots
            .iter()
            .filter(|snapshot| !keep.contains(&snapshot.id))
            .map(|snapshot| snapshot.id.clone())
            .collect();
        if !targets.is_empty() && requires_compaction(&snapshots, &keep, &heads)? {
            let head = heads
                .first()
                .context("Save history has no current head; history was not pruned")?;
            // Publish the shorter lineage before deleting anything. This is a
            // regular save snapshot understood by existing readers. A crash
            // leaves either the old complete history or the compacted history;
            // no retained ancestor can become a spurious competing head.
            let spinner = create_spinner(format!("{}: Compacting save history...", id.label()));
            let mut tags: Vec<_> = head
                .tags
                .iter()
                .filter(|tag| {
                    !tag.starts_with("instantgame-parent:")
                        && !tag.starts_with("instantgame-preserve:")
                        && !tag.starts_with("instantgame-save-time:")
                        && *tag != "rewrite"
                })
                .cloned()
                .collect();
            tags.extend(compaction_tags(&snapshots, &keep));
            tags.push(format!(
                "instantgame-save-time:{}",
                save_runtime::snapshot_save_time(head)?.to_rfc3339()
            ));
            tags.sort_unstable();
            tags.dedup();
            let result = restic.copy_snapshot_with_tags(head, &tags);
            spinner.finish_and_clear();
            let compacted_id =
                result.context("Compacting save history; original snapshots were not removed")?;
            let observed = save_runtime::fresh_snapshots(&config, &id)?;
            let observed_heads = save_runtime::history_heads(&observed)?;
            ensure!(
                observed_heads.len() == 1 && observed_heads[0].id == compacted_id,
                "Save history changed during compaction; original snapshots were not removed. Sync saves before retrying pruning"
            );
        }
        restic
            .forget_snapshots(&targets, true)
            .context("Pruning save history")?;
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

/// A compacted head directly supersedes every retained snapshot. Carry recovery
/// pins forward even when the snapshot that originally recorded them expires.
fn compaction_tags(snapshots: &[Snapshot], keep: &HashSet<String>) -> Vec<String> {
    let retained: Vec<_> = snapshots
        .iter()
        .filter(|snapshot| keep.contains(&snapshot.id))
        .cloned()
        .collect();
    let mut tags = save_runtime::parent_tags(&retained);
    tags.push("instantgame-compaction".into());
    let preserved: HashSet<_> = snapshots
        .iter()
        .flat_map(|snapshot| &snapshot.tags)
        .filter_map(|tag| tag.strip_prefix("instantgame-preserve:"))
        .filter(|id| keep.contains(*id))
        .collect();
    let mut preserved: Vec<_> = preserved.into_iter().collect();
    preserved.sort_unstable();
    tags.extend(
        preserved
            .into_iter()
            .map(|id| format!("instantgame-preserve:{id}")),
    );
    tags
}

fn requires_compaction(
    snapshots: &[Snapshot],
    keep: &HashSet<String>,
    heads: &[Snapshot],
) -> Result<bool> {
    let retained: Vec<_> = snapshots
        .iter()
        .filter(|snapshot| keep.contains(&snapshot.id))
        .cloned()
        .collect();
    let retained_heads = save_runtime::history_heads(&retained)?;
    if retained_heads.len() != heads.len()
        || retained_heads
            .iter()
            .any(|candidate| !heads.iter().any(|head| head.id == candidate.id))
    {
        return Ok(true);
    }
    // A recovery pin must outlive the snapshot that originally recorded it.
    let retained_tags: HashSet<_> = retained
        .iter()
        .flat_map(|snapshot| &snapshot.tags)
        .collect();
    Ok(compaction_tags(snapshots, keep)
        .iter()
        .filter(|tag| tag.starts_with("instantgame-preserve:"))
        .any(|tag| !retained_tags.contains(tag)))
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
    fn pruning_removes_intermediates_without_turning_pinned_checkpoints_into_branches() -> Result<()>
    {
        let history = vec![
            snapshot("a", "root"),
            snapshot("b", "a"),
            snapshot("c", "b"),
            snapshot("d", "c"),
        ];
        let keep = HashSet::from(["b".into(), "d".into()]);
        let mut compacted = snapshot("compacted", "root");
        compacted.tags = compaction_tags(&history, &keep);
        let mut retained: Vec<_> = history
            .into_iter()
            .filter(|snapshot| keep.contains(&snapshot.id))
            .collect();
        retained.push(compacted);
        assert_eq!(
            save_runtime::history_heads(&retained)?
                .iter()
                .map(|snapshot| snapshot.id.as_str())
                .collect::<Vec<_>>(),
            ["compacted"]
        );
        // A concurrent upload remains visible as a real independent branch.
        retained.push(snapshot("concurrent", "d"));
        assert_eq!(save_runtime::history_heads(&retained)?.len(), 2);
        Ok(())
    }

    #[test]
    fn compaction_keeps_recovery_pins_when_their_original_reference_expires() {
        let mut merged = snapshot("merged", "recovery");
        merged.tags.push("instantgame-preserve:recovery".into());
        let history = vec![
            snapshot("recovery", "root"),
            merged,
            snapshot("head", "merged"),
        ];
        let keep = HashSet::from(["recovery".into(), "head".into()]);
        let tags = compaction_tags(&history, &keep);
        assert!(tags.contains(&"instantgame-preserve:recovery".into()));
        assert!(!tags.contains(&"instantgame-parent:merged".into()));
    }
}
