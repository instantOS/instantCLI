use anyhow::{Context, Result};

use super::config::InstallationsConfig;
use super::restic::cache;

/// Helper function to update installation checkpoint
fn update_local(game_name: &str, checkpoint_id: &str) -> Result<()> {
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Find and update the installation
    for installation in &mut installations.installations {
        if installation.game_name.0 == game_name {
            installation.note_backup(checkpoint_id);
            break;
        }
    }

    installations
        .save()
        .context("Failed to save updated installations configuration")
}

/// Resolve the checkpoint produced by a backup without parsing display text.
pub fn resolve_backup_snapshot_id(
    snapshot_id: Option<&str>,
    game_name: &str,
    game_config: &super::config::InstantGameConfig,
) -> Result<Option<String>> {
    if let Some(snapshot_id) = snapshot_id {
        Ok(Some(snapshot_id.to_string()))
    } else {
        cache::invalidate_snapshot_cache();
        // Try to get the latest snapshot for this game as fallback
        match cache::get_snapshots_for_game(game_name, game_config) {
            Ok(snapshots) => {
                if let Some(latest) = snapshots.first() {
                    Ok(Some(latest.id.clone()))
                } else {
                    eprintln!("Warning: Could not determine snapshot ID for checkpoint update");
                    Ok(None)
                }
            }
            Err(e) => {
                eprintln!("Warning: Could not fetch snapshots for checkpoint update: {e}");
                Ok(None)
            }
        }
    }
}

/// Update installation checkpoint after successful backup
pub fn update_checkpoint_after_backup(
    snapshot_id: Option<&str>,
    game_name: &str,
    game_config: &super::config::InstantGameConfig,
) -> Result<()> {
    if let Some(snapshot_id) = resolve_backup_snapshot_id(snapshot_id, game_name, game_config)? {
        update_local(game_name, &snapshot_id)
            .context("Could not update checkpoint after backup")?;
    }
    Ok(())
}

/// Persist the restore intent before any operation can alter live save files.
pub fn mark_restore_pending(game_name: &str, snapshot_id: &str) -> Result<()> {
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;
    let installation = installations
        .installations
        .iter_mut()
        .find(|installation| installation.game_name.0 == game_name)
        .with_context(|| format!("No installation configured for game '{game_name}'"))?;
    installation.pending_restore = Some(snapshot_id.to_string());
    installations
        .save()
        .context("Could not save restore retry state; no files were restored")
}

/// Complete a restore in one durable config write.
///
/// Keeping the checkpoint, acknowledged remote head, and pending marker in the
/// same transition prevents a crash from making an explicit historical restore
/// look like an ordinary stale checkpoint.
pub fn complete_restore(game_name: &str, snapshot_id: &str) -> Result<()> {
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Get the snapshot to extract its actual timestamp
    let game_config =
        super::config::InstantGameConfig::load().context("Failed to load game configuration")?;
    let snapshot = cache::get_snapshot_by_id(snapshot_id, game_name, &game_config)?
        .context("Snapshot not found for checkpoint update")?;

    // Acknowledge the remote head unless the restore targeted it: a head that
    // matches the checkpoint was restored, not superseded.
    let acknowledged_head = cache::get_snapshots_for_game(game_name, &game_config)?
        .first()
        .filter(|head| !head.matches_id(&snapshot.id))
        .map(|head| head.id.clone());

    let installation = installations
        .installations
        .iter_mut()
        .find(|installation| installation.game_name.0 == game_name)
        .with_context(|| format!("No installation configured for game '{game_name}'"))?;
    // Store the resolved full snapshot ID, never the user-supplied (possibly
    // short) form, so checkpoint comparisons stay exact.
    installation.note_restored_snapshot(snapshot.id.clone(), snapshot.time, acknowledged_head);

    installations
        .save()
        .context("Failed to save completed restore state")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_backup_snapshot_id_is_not_parsed_as_display_text() -> Result<()> {
        let config = super::super::config::InstantGameConfig::default();
        assert_eq!(
            resolve_backup_snapshot_id(Some("snapshot-id"), "game", &config)?,
            Some("snapshot-id".to_string())
        );
        Ok(())
    }
}
