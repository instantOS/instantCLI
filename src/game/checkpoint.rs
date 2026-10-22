use anyhow::{Context, Result};

use super::config::InstallationsConfig;
use super::ownership::OwnerId;

/// Persist the restore intent before any operation can alter live save files.
pub fn mark_owner_restore_pending(owner: &OwnerId, snapshot_id: &str) -> Result<()> {
    InstallationsConfig::update_owner(owner, |saves| {
        saves.pending_restore = Some(snapshot_id.into());
        Ok(())
    })
    .context("Could not save restore retry state; no files were restored")
}

/// Complete a restore in one durable config write.
///
/// Keeping the checkpoint, acknowledged remote head, and pending marker in the
/// same transition prevents a crash from making an explicit historical restore
/// look like an ordinary stale checkpoint.
pub fn complete_owner_restore(
    owner: &OwnerId,
    snapshot_id: &str,
    snapshot_time: &str,
    observed_head: Option<&str>,
    game_config: &super::config::InstantGameConfig,
) -> Result<()> {
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    let acknowledged_head = observed_head
        .filter(|head| *head != snapshot_id)
        .map(str::to_owned);

    let content =
        super::save_runtime::content_digest(installations.owner_saves(owner)?.save_path.as_path())?;
    InstallationsConfig::update_owner(owner, |saves| {
        saves.note_restored_snapshot(snapshot_id, snapshot_time, acknowledged_head);
        saves.baseline = Some(super::config::SaveBaseline {
            content,
            remote_head: observed_head.map(str::to_owned),
        });
        saves.sync_repository = Some(game_config.repo.as_path().to_string_lossy().into_owned());
        Ok(())
    })
    .context("Failed to save completed restore state")
}
