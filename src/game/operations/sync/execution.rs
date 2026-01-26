use crate::game::checkpoint;
use crate::game::config::{GameInstallation, InstantGameConfig};
use crate::game::restic::backup::{GameBackup, RestoreRequest};
use crate::game::restic::cache;
use anyhow::{Context, Result};

/// Create backup for a game
pub fn perform_backup(
    installation: &GameInstallation,
    game_config: &InstantGameConfig,
) -> Result<()> {
    let backup_handler = GameBackup::new(game_config.clone());

    let backup_result = backup_handler
        .backup_game(installation)
        .context("Failed to create backup")?;

    // Update checkpoint after successful backup
    checkpoint::update_checkpoint_after_backup(
        &backup_result,
        &installation.game_name.0,
        game_config,
    )?;

    // Invalidate cache for this game after successful backup
    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(&installation.game_name.0, &repo_path);

    Ok(())
}

/// Restore game from snapshot
pub fn perform_restore(
    installation: &GameInstallation,
    game_config: &InstantGameConfig,
    snapshot_id: &str,
) -> Result<()> {
    let backup_handler = GameBackup::new(game_config.clone());
    let save_path = installation.save_path.as_path();
    let snapshot_hint =
        cache::get_snapshot_by_id(snapshot_id, &installation.game_name.0, game_config)
            .ok()
            .and_then(|snapshot| snapshot.and_then(|snap| snap.paths.first().cloned()));

    // Use the appropriate restore method based on save path type
    backup_handler
        .restore_backup(RestoreRequest {
            game_name: &installation.game_name.0,
            snapshot_id,
            path: save_path,
            save_path_type: installation.save_path_type,
            snapshot_source_path: snapshot_hint.as_deref(),
        })
        .context("Failed to restore from snapshot")?;

    // Update the installation with the checkpoint
    checkpoint::update_checkpoint_after_restore(&installation.game_name.0, snapshot_id)?;

    // Invalidate cache for this game after successful restore
    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(&installation.game_name.0, &repo_path);

    Ok(())
}
