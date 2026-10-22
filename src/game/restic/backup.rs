use anyhow::{Context, Result};
use std::{fs, path::Path};

use crate::game::checkpoint;
use crate::game::config::{InstantGameConfig, PathContentKind};
use crate::game::ownership::{OwnerId, SaveOwner};
use crate::game::restic::{cache, single_file};
use crate::restic::ResticWrapper;

/// Request parameters for restoring a game backup
pub struct RestoreRequest<'a> {
    pub owner: &'a OwnerId,
    pub snapshot_id: &'a str,
    pub snapshot_time: &'a str,
    pub path: &'a Path,
    pub save_path_type: PathContentKind,
    /// Optional hint for the snapshot source path (from cached snapshot metadata)
    pub snapshot_source_path: Option<&'a str>,
    pub observed_head: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupResult {
    pub snapshot_id: Option<String>,
    pub file_contents_changed: bool,
    pub content_digest: String,
}

impl std::fmt::Display for BackupResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.snapshot_id, self.file_contents_changed) {
            (Some(snapshot_id), true) => write!(formatter, "snapshot: {snapshot_id}"),
            (Some(snapshot_id), false) => {
                write!(formatter, "snapshot: {snapshot_id} (no file changes)")
            }
            (None, false) => formatter
                .write_str("backup completed (no snapshot created; no file changes detected)"),
            (None, true) => formatter.write_str("backup completed (no snapshot created)"),
        }
    }
}

/// Backup game saves to restic repository with proper tagging
pub struct GameBackup {
    pub config: InstantGameConfig,
}

impl GameBackup {
    pub fn new(config: InstantGameConfig) -> Self {
        Self { config }
    }

    pub(crate) fn backup_owner(
        &self,
        owner: &SaveOwner,
        extra_tags: &[String],
    ) -> Result<BackupResult> {
        // Validate that save path exists
        let save_path_buf = owner.save_path.as_path();
        if !save_path_buf.exists() {
            return Err(anyhow::anyhow!(
                "Save path does not exist: {}",
                save_path_buf.display()
            ));
        }

        if owner.save_path_type.is_directory() && !save_path_buf.is_dir() {
            return Err(anyhow::anyhow!(
                "Configured save path '{}' is not a directory",
                save_path_buf.display()
            ));
        }

        if owner.save_path_type.is_file() && !save_path_buf.is_file() {
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

        // Save lineage requires a snapshot even when restic detects no changes.
        let content = crate::game::save_runtime::content_digest(save_path_buf)?;
        let modified = crate::game::save_runtime::latest_save_time(save_path_buf)?
            .context("Save path has no modification time")?;
        let mut tags = owner.id.tags();
        tags.push(format!("instantgame-content:{content}"));
        tags.push(format!("instantgame-save-time:{}", modified.to_rfc3339()));
        tags.extend_from_slice(extra_tags);

        let restic_paths: Vec<&Path> = match owner.save_path_type {
            PathContentKind::Directory => vec![save_path_buf],
            PathContentKind::File => {
                // For single files, backup the file directly
                vec![save_path_buf]
            }
        };

        cache::invalidate_snapshot_cache();
        let progress = restic
            .backup_saves(&restic_paths, tags)
            .context("Failed to perform restic backup")?;

        anyhow::ensure!(
            content == crate::game::save_runtime::content_digest(save_path_buf)?,
            "Saves changed during backup. Snapshot retained; local baseline was not advanced"
        );
        let summary = progress
            .summary
            .context("Restic backup returned no summary")?;
        Ok(BackupResult {
            snapshot_id: Some(
                summary
                    .snapshot_id
                    .context("Restic backup did not return a snapshot ID")?,
            ),
            file_contents_changed: summary.files_new != 0 || summary.files_changed != 0,
            content_digest: content,
        })
    }

    /// Restore a game backup
    fn restore_game_backup(
        &self,
        snapshot_id: &str,
        target_path: &Path,
        source_path: Option<&str>,
    ) -> Result<String> {
        if !target_path.exists() {
            fs::create_dir_all(target_path).with_context(|| {
                format!("Failed to create restore target: {}", target_path.display())
            })?;
        }

        // The owner was checked when selecting this snapshot. Reuse its exact
        // source path rather than querying repository metadata again.
        let snapshot_path = source_path
            .filter(|path| !path.is_empty())
            .context("Snapshot has no source directory; refusing an unscoped restore")?;

        let restic = ResticWrapper::new(
            self.config.repo.as_path().to_string_lossy().to_string(),
            self.config.repo_password.clone(),
        )
        .context("Failed to initialize restic wrapper")?;

        let progress = restic
            .restore(snapshot_id, Some(snapshot_path), target_path)
            .context("Failed to restore restic snapshot")?;

        if let Some(summary) = progress.summary {
            return Ok(format!("restored {} files", summary.files_restored));
        }

        Ok("restore completed".to_string())
    }

    /// Restore a game backup (handles both files and directories)
    pub fn restore_backup(&self, request: RestoreRequest<'_>) -> Result<String> {
        checkpoint::mark_owner_restore_pending(request.owner, request.snapshot_id)?;
        let summary = self.restore_untracked(&request).with_context(|| {
            format!(
                "Restore incomplete. Do not play '{}'; retry the restore before syncing",
                request.owner.label()
            )
        })?;
        checkpoint::complete_owner_restore(
            request.owner,
            request.snapshot_id,
            request.snapshot_time,
            request.observed_head,
            &self.config,
        )
        .context("Restore completed, but its state could not be saved; retry before syncing")?;
        Ok(summary)
    }

    pub(crate) fn restore_untracked(&self, request: &RestoreRequest<'_>) -> Result<String> {
        match request.save_path_type {
            PathContentKind::Directory => {
                // For directories, use the standard restore
                let summary = self.restore_game_backup(
                    request.snapshot_id,
                    request.path,
                    request.snapshot_source_path,
                )?;
                Ok(summary)
            }
            PathContentKind::File => {
                let restic = ResticWrapper::new(
                    self.config.repo.as_path().to_string_lossy().to_string(),
                    self.config.repo_password.clone(),
                )
                .context("Failed to initialize restic wrapper")?;

                let temporary = tempfile::tempdir()
                    .context("Creating isolated single-file restore directory")?;
                let temp_restore = temporary.path();

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
                    temp_restore,
                    request.path,
                )?;

                // Ensure target parent directory exists
                if let Some(parent) = request.path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create target directory: {}", parent.display())
                    })?;
                }

                // Get the modification time from the restored file before copying
                let source_mtime = fs::metadata(&restored_file)
                    .and_then(|metadata| metadata.modified())
                    .context("Reading restored save modification time")?;

                // Move the file to final location
                fs::copy(&restored_file, request.path).with_context(|| {
                    format!(
                        "Failed to copy restored file from {} to {}",
                        restored_file.display(),
                        request.path.display()
                    )
                })?;

                fs::File::open(request.path)?
                    .set_times(std::fs::FileTimes::new().set_modified(source_mtime))
                    .context("Preserving restored save modification time")?;

                Ok(single_file::summarize_restore(&progress)
                    .unwrap_or_else(|| "restore completed".to_string()))
            }
        }
    }

    /// Require the restore semantics used by game-save reconciliation.
    pub fn validate_restic_version() -> Result<()> {
        let restic = ResticWrapper::new(String::new(), String::new())?;
        let version = restic.installed_version()?;
        anyhow::ensure!(
            version >= (0, 17, 0),
            "restic 0.17.0 or newer is required; found {}.{}.{}",
            version.0,
            version.1,
            version.2
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::BackupResult;

    #[test]
    fn backup_result_keeps_snapshot_id_separate_from_display_summary() {
        let result = BackupResult {
            snapshot_id: Some("snapshot-id".into()),
            file_contents_changed: false,
            content_digest: "hash".into(),
        };

        assert_eq!(result.snapshot_id.as_deref(), Some("snapshot-id"));
        assert_eq!(
            result.to_string(),
            "snapshot: snapshot-id (no file changes)"
        );
    }
}
