use anyhow::{Context, Result};
use std::{fs, path::Path};

use crate::game::config::{GameInstallation, InstantGameConfig, PathContentKind};
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
        let save_path_buf = game_installation.save_path.as_path();
        if !save_path_buf.exists() {
            return Err(anyhow::anyhow!(
                "Save path does not exist: {}",
                save_path_buf.display()
            ));
        }

        if game_installation.save_path_type.is_directory() && !save_path_buf.is_dir() {
            return Err(anyhow::anyhow!(
                "Configured save path '{}' is not a directory",
                save_path_buf.display()
            ));
        }

        if game_installation.save_path_type.is_file() && !save_path_buf.is_file() {
            return Err(anyhow::anyhow!(
                "Configured save path '{}' is not a file",
                save_path_buf.display()
            ));
        }

        let restic = ResticWrapper::new(
            self.config.repo.as_path().to_string_lossy().to_string(),
            self.config.repo_password.clone(),
        );

        // Use the centralized restic wrapper which already includes
        // --skip-if-unchanged for backups.
        let tags = tags::create_game_tags(&game_installation.game_name.0);

        let restic_paths: Vec<&Path> = match game_installation.save_path_type {
            PathContentKind::Directory => vec![save_path_buf],
            PathContentKind::File => {
                // For single files, backup the file directly
                vec![save_path_buf]
            }
        };

        let progress = if game_installation.save_path_type.is_file() {
            // For single files, use standard backup (no include filter needed)
            restic.backup(&restic_paths, tags)
                .context("Failed to perform restic backup for single file")?
        } else {
            // For directories, use standard backup
            restic.backup(&restic_paths, tags)
                .context("Failed to perform restic backup for directory")?
        };

        if let Some(summary) = progress.summary {
            let snapshot_id = summary.snapshot_id.clone();
            let no_file_changes = summary.files_new == 0 && summary.files_changed == 0;

            if let Some(snap) = snapshot_id {
                if no_file_changes {
                    return Ok(format!("snapshot: {snap} (no file changes)"));
                }
                return Ok(format!("snapshot: {snap}"));
            }

            if no_file_changes {
                return Ok(
                    "backup completed (no snapshot created; no file changes detected)".to_string(),
                );
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

        let snapshot = cache::get_snapshot_by_id(snapshot_id, game_name, &self.config)
            .context("Failed to locate snapshot metadata")?;

        let snapshot_path = snapshot.and_then(|s| s.paths.first().cloned());

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

    /// Restore a game backup with single file support
    pub fn restore_game_backup_with_type(
        &self,
        game_name: &str,
        snapshot_id: &str,
        target_path: &Path,
        save_path_type: PathContentKind,
        original_save_path: &Path,
    ) -> Result<String> {
        match save_path_type {
            PathContentKind::Directory => {
                // For directories, use the standard restore
                self.restore_game_backup(game_name, snapshot_id, target_path)
            }
            PathContentKind::File => {
                // For single files, we need to restore just the specific file
                if let Some(parent) = target_path.parent() {
                    if !parent.exists() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!("Failed to create restore target parent: {}", parent.display())
                        })?;
                    }
                }

                let restic = ResticWrapper::new(
                    self.config.repo.as_path().to_string_lossy().to_string(),
                    self.config.repo_password.clone(),
                );

                // Use restore_single_file to restore just the specific file
                let progress = restic
                    .restore_single_file(snapshot_id, &original_save_path.to_string_lossy(), target_path.parent().unwrap_or(target_path))
                    .context("Failed to restore single file from snapshot")?;

                if let Some(summary) = progress.summary {
                    return Ok(format!("restored {} files", summary.files_restored));
                }

                Ok("restore completed".to_string())
            }
        }
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
