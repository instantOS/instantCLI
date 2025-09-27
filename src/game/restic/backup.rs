use anyhow::{Context, Result};
use std::{fs, path::Path};

use crate::game::config::{GameInstallation, InstantGameConfig};
use crate::game::restic::{cache, tags};
use crate::restic::ResticWrapper;

/// Backup game saves to restic repository with proper tagging
pub struct GameBackup {
    pub config: InstantGameConfig,
}

impl GameBackup {
    pub fn new(config: InstantGameConfig) -> Self {
        Self { config }
    }

    /// Create a backup of a specific game's save directory
    pub fn backup_game(&self, game_installation: &GameInstallation) -> Result<String> {
        // Validate that save path exists
        if !game_installation.save_path.as_path().exists() {
            return Err(anyhow::anyhow!(
                "Save path does not exist: {}",
                game_installation.save_path.as_path().display()
            ));
        }

        let restic = ResticWrapper::new(
            self.config.repo.as_path().to_string_lossy().to_string(),
            self.config.repo_password.clone(),
        );

        // Use the centralized restic wrapper which already includes
        // --skip-if-unchanged for backups.
        let tags = tags::create_game_tags(&game_installation.game_name.0);

        let progress = restic
            .backup(&[game_installation.save_path.as_path()], tags)
            .context("Failed to perform restic backup")?;

        if let Some(summary) = progress.summary {
            if let Some(snap) = summary.snapshot_id {
                return Ok(format!("snapshot: {snap}"));
            }
        }

        Ok("backup completed (no snapshot created)".to_string())
    }

    /// List backups for a specific game
    pub fn list_game_backups(&self, game_name: &str) -> Result<String> {
        let restic = ResticWrapper::new(
            self.config.repo.as_path().to_string_lossy().to_string(),
            self.config.repo_password.clone(),
        );

        let json = restic
            .list_snapshots_filtered(Some(tags::create_game_tags(game_name)))
            .context("Failed to list restic snapshots")?;

        Ok(json)
    }

    /// Restore a game backup
    pub fn restore_game_backup(
        &self,
        game_name: &str,
        snapshot_id: &str,
        target_path: &Path,
    ) -> Result<String> {
        if !target_path.exists() {
            fs::create_dir_all(target_path).with_context(|| {
                format!("Failed to create restore target: {}", target_path.display())
            })?;
        }

        let snapshot_path = cache::get_snapshot_by_id(snapshot_id, game_name, &self.config)
            .context("Failed to locate snapshot metadata")?
            .and_then(|snapshot| snapshot.paths.first().cloned());

        let restic = ResticWrapper::new(
            self.config.repo.as_path().to_string_lossy().to_string(),
            self.config.repo_password.clone(),
        );

        let progress = restic
            .restore(snapshot_id, snapshot_path.as_deref(), target_path)
            .context("Failed to restore restic snapshot")?;

        if let Some(summary) = progress.summary {
            return Ok(format!("restored {} files", summary.files_restored));
        }

        Ok("restore completed".to_string())
    }

    /// Check if restic is available on the system
    pub fn check_restic_availability() -> Result<bool> {
        // Use the wrapper to query version
        let restic = ResticWrapper::new("".to_string(), "".to_string());
        match restic.check_version() {
            Ok(success) => Ok(success),
            Err(_) => Ok(false),
        }
    }
}
