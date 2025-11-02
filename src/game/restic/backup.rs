use anyhow::{Context, Result, anyhow};
use std::{fs, path::Path};
use walkdir::WalkDir;

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
            restic
                .backup(&restic_paths, tags)
                .context("Failed to perform restic backup for single file")?
        } else {
            // For directories, use standard backup
            restic
                .backup(&restic_paths, tags)
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

    /// Restore a game backup (handles both files and directories)
    pub fn restore_backup(
        &self,
        game_name: &str,
        snapshot_id: &str,
        target_path: &Path,
        save_path_type: PathContentKind,
        original_save_path: &Path,
        snapshot_source_path: Option<&str>,
    ) -> Result<String> {
        match save_path_type {
            PathContentKind::Directory => {
                // For directories, use the standard restore
                self.restore_game_backup(game_name, snapshot_id, target_path)
            }
            PathContentKind::File => {
                let restic = ResticWrapper::new(
                    self.config.repo.as_path().to_string_lossy().to_string(),
                    self.config.repo_password.clone(),
                );

                let include_path = snapshot_source_path
                    .map(|path| path.to_string())
                    .unwrap_or_else(|| original_save_path.to_string_lossy().into_owned());

                // For single files, restore to temp directory then move to final location
                let temp_restore = std::env::temp_dir().join(format!(
                    "ins-restore-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()
                ));

                fs::create_dir_all(&temp_restore).with_context(|| {
                    format!(
                        "Failed to create temp restore directory: {}",
                        temp_restore.display()
                    )
                })?;

                // Restore to temp directory
                let progress = restic
                    .restore_single_file(snapshot_id, &include_path, &temp_restore)
                    .context("Failed to restore single file from snapshot")?;

                // Find the restored file in the temp directory
                let restored_file = Self::find_restored_file(&temp_restore, original_save_path)?;

                // Ensure target parent directory exists
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create target directory: {}", parent.display())
                    })?;
                }

                // Move the file to final location
                fs::copy(&restored_file, target_path).with_context(|| {
                    format!(
                        "Failed to copy restored file from {} to {}",
                        restored_file.display(),
                        target_path.display()
                    )
                })?;

                // Cleanup temp directory
                let _ = fs::remove_dir_all(&temp_restore);

                let restored_files = progress
                    .summary
                    .map(|summary| summary.files_restored)
                    .unwrap_or(1);

                Ok(format!(
                    "restored {restored_files} file{}",
                    if restored_files == 1 { "" } else { "s" }
                ))
            }
        }
    }
    fn find_restored_file(
        temp_dir: &Path,
        original_save_path: &Path,
    ) -> Result<std::path::PathBuf> {
        let original_name = original_save_path.file_name();
        let mut candidates = Vec::new();

        for entry in WalkDir::new(temp_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.into_path();
            if original_name.is_some() && path.file_name() == original_name {
                return Ok(path);
            }
            candidates.push(path);
        }

        match candidates.len() {
            0 => Err(anyhow!(
                "restic restore did not produce any files under {}",
                temp_dir.display()
            )),
            1 => Ok(candidates.into_iter().next().unwrap()),
            _ => {
                // Prefer the shallowest path (closest to the root)
                candidates
                    .into_iter()
                    .min_by_key(|path| path.components().count())
                    .ok_or_else(|| anyhow!("restic restore produced multiple files unexpectedly"))
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
