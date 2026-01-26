use anyhow::{Context, Result};
use std::{fs, path::Path};

use crate::game::config::{GameInstallation, InstantGameConfig, PathContentKind};
use crate::game::restic::{cache, single_file, tags};
use crate::restic::ResticWrapper;

/// Request parameters for restoring a game backup
pub struct RestoreRequest<'a> {
    pub game_name: &'a str,
    pub snapshot_id: &'a str,
    pub path: &'a Path,
    pub save_path_type: PathContentKind,
    /// Optional hint for the snapshot source path (from cached snapshot metadata)
    pub snapshot_source_path: Option<&'a str>,
}

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
        )
        .context("Failed to initialize restic wrapper")?;

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
        )
        .context("Failed to initialize restic wrapper")?;

        let progress = restic
            .restore(snapshot_id, snapshot_path.as_deref(), target_path)
            .context("Failed to restore restic snapshot")?;

        if let Some(summary) = progress.summary {
            return Ok(format!("restored {} files", summary.files_restored));
        }

        Ok("restore completed".to_string())
    }

    /// Restore a game backup (handles both files and directories)
    pub fn restore_backup(&self, request: RestoreRequest<'_>) -> Result<String> {
        match request.save_path_type {
            PathContentKind::Directory => {
                // For directories, use the standard restore
                let summary =
                    self.restore_game_backup(request.game_name, request.snapshot_id, request.path)?;
                Ok(summary)
            }
            PathContentKind::File => {
                let restic = ResticWrapper::new(
                    self.config.repo.as_path().to_string_lossy().to_string(),
                    self.config.repo_password.clone(),
                )
                .context("Failed to initialize restic wrapper")?;

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

                let mut candidate_paths = Vec::new();
                if let Some(source_path) = request.snapshot_source_path {
                    candidate_paths.push(source_path.to_string());
                }

                let resolved_snapshot_path = single_file::resolve_snapshot_file_path(
                    &restic,
                    request.snapshot_id,
                    &candidate_paths,
                    Some(request.path),
                )?;

                // Restore to temp directory
                let (restored_file, progress) = single_file::restore_single_file_into_temp(
                    &restic,
                    request.snapshot_id,
                    &resolved_snapshot_path,
                    &temp_restore,
                    request.path,
                )?;

                // Ensure target parent directory exists
                if let Some(parent) = request.path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create target directory: {}", parent.display())
                    })?;
                }

                // Get the modification time from the restored file before copying
                let source_mtime = fs::metadata(&restored_file).and_then(|m| m.modified()).ok();

                // Move the file to final location
                fs::copy(&restored_file, request.path).with_context(|| {
                    format!(
                        "Failed to copy restored file from {} to {}",
                        restored_file.display(),
                        request.path.display()
                    )
                })?;

                // Preserve the original modification time after copy
                if let Some(mtime) = source_mtime
                    && let Ok(file) = fs::File::options().write(true).open(request.path)
                {
                    let times = std::fs::FileTimes::new().set_modified(mtime);
                    let _ = file.set_times(times);
                }

                // Cleanup temp directory
                let _ = fs::remove_dir_all(&temp_restore);

                Ok(single_file::summarize_restore(&progress)
                    .unwrap_or_else(|| "restore completed".to_string()))
            }
        }
    }
    /// Check if restic is available on the system
    pub fn check_restic_availability() -> Result<bool> {
        // Use the wrapper to query version
        let restic = ResticWrapper::new("".to_string(), "".to_string());
        match restic {
            Ok(r) => match r.check_version() {
                Ok(success) => Ok(success),
                Err(_) => Ok(false),
            },
            Err(_) => Ok(false),
        }
    }
}
