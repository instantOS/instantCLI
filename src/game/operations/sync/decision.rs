use anyhow::{Context, Result};

use super::types::SyncAction;
use crate::game::config::InstantGameConfig;
use crate::game::ownership::SaveOwner;
use crate::game::save_runtime::{self, BaselineAction};

pub fn determine_action(
    owner: &SaveOwner,
    config: &InstantGameConfig,
    force: bool,
    view: &mut crate::game::restic::cache::SnapshotView,
) -> Result<SyncAction> {
    anyhow::ensure!(
        !owner.needs_repository_reconciliation(&config.repo.as_path().to_string_lossy()),
        "Save repository changed; finish game setup before playing"
    );
    let snapshots = view.for_owner(config, &owner.id)?;
    if let Some(pending) = &owner.pending_restore {
        let snapshot = snapshots.iter().find(|snapshot| snapshot.matches_id(pending)).context("The incomplete restore snapshot is unavailable; explicitly reconcile saves before playing")?;
        return Ok(SyncAction::RestoreFromSnapshot(snapshot.id.clone()));
    }
    let heads = save_runtime::history_heads(&snapshots)?;
    if heads.len() > 1 {
        return Ok(SyncAction::ResolveNewest);
    }
    let local = save_runtime::content_digest(owner.save_path.as_path())?;
    if owner.baseline.is_none() && !save_runtime::has_save_data(owner.save_path.as_path())? {
        return Ok(match heads.first() {
            Some(head) => SyncAction::RestoreFromSnapshot(head.id.clone()),
            None => SyncAction::NoActionNeeded,
        });
    }
    if let Some(head) = heads.first()
        && save_runtime::snapshot_digest(head) == Some(local.as_str())
        && owner
            .baseline
            .as_ref()
            .is_none_or(|baseline| baseline.remote_head.as_deref() != Some(head.id.as_str()))
    {
        return Ok(SyncAction::AcknowledgeSnapshot(head.id.clone()));
    }
    match save_runtime::baseline_action(owner, &local, heads.first().map(|head| head.id.as_str())) {
        BaselineAction::Skip if force => Ok(SyncAction::CreateBackup),
        BaselineAction::Skip => Ok(SyncAction::NoActionNeeded),
        BaselineAction::Backup => Ok(SyncAction::CreateBackup),
        BaselineAction::Restore(id) => Ok(SyncAction::RestoreFromSnapshot(id)),
        BaselineAction::Reconcile => Ok(SyncAction::ResolveNewest),
    }
}
