use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::game::config::{DependencyKind, GameDependency, InstantGameConfig};
use crate::game::restic::{cache, tags};
use crate::restic::wrapper::{BackupProgress, ResticWrapper, RestoreProgress, Snapshot};

/// Result of creating or reusing a dependency snapshot
pub struct DependencyBackupResult {
    pub snapshot_id: String,
    pub reused_existing: bool,
    pub progress: BackupProgress,
}

/// Result of restoring a dependency snapshot
pub struct DependencyRestoreResult {
    pub snapshot_id: String,
    pub progress: RestoreProgress,
}

/// List snapshots for a dependency filtered by tags
pub fn list_dependency_snapshots(
    game_name: &str,
    dependency_id: &str,
    config: &InstantGameConfig,
) -> Result<Vec<Snapshot>> {
    let required_tags = tags::create_dependency_tags(game_name, dependency_id);

    let mut snapshots: Vec<Snapshot> = cache::get_repository_snapshots(config)?
        .into_iter()
        .filter(|snapshot| {
            required_tags
                .iter()
                .all(|tag| snapshot.tags.iter().any(|existing| existing == tag))
        })
        .collect();

    snapshots.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(snapshots)
}

/// Create or reuse dependency snapshot in restic repository
pub fn backup_dependency(
    game_name: &str,
    dependency_id: &str,
    source_path: &Path,
    config: &InstantGameConfig,
) -> Result<DependencyBackupResult> {
    if !source_path.exists() {
        return Err(anyhow!(
            "Dependency source path does not exist: {}",
            source_path.display()
        ));
    }

    let restic = ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    );

    let tags = tags::create_dependency_tags(game_name, dependency_id);
    let progress = restic.backup(&[source_path], tags).with_context(|| {
        format!(
            "Failed to backup dependency '{}:{}'",
            game_name, dependency_id
        )
    })?;

    // Ensure cache is refreshed so we can discover new snapshots
    cache::invalidate_snapshot_cache();

    let reused_existing = progress
        .summary
        .as_ref()
        .map(|summary| summary.files_new == 0 && summary.files_changed == 0)
        .unwrap_or(false);

    let snapshot_id = if let Some(summary) = progress.summary.as_ref()
        && let Some(id) = &summary.snapshot_id
    {
        id.clone()
    } else {
        let snapshots = list_dependency_snapshots(game_name, dependency_id, config)?;
        snapshots
            .first()
            .map(|snapshot| snapshot.id.clone())
            .ok_or_else(|| anyhow!("Failed to locate snapshot for dependency after backup"))?
    };

    Ok(DependencyBackupResult {
        snapshot_id,
        reused_existing,
        progress,
    })
}

/// Restore dependency snapshot content into target path (files or directories)
pub fn restore_dependency(
    game_name: &str,
    dependency: &GameDependency,
    config: &InstantGameConfig,
    target_path: &Path,
) -> Result<DependencyRestoreResult> {
    let source_path = PathBuf::from(&dependency.source_path);
    let snapshot_id = if let Some(id) = &dependency.snapshot_id {
        id.clone()
    } else {
        let snapshots = list_dependency_snapshots(game_name, &dependency.id, config)?;
        snapshots
            .first()
            .map(|snapshot| snapshot.id.clone())
            .ok_or_else(|| {
                anyhow!(
                    "No snapshot found for dependency '{}:{}'",
                    game_name,
                    dependency.id
                )
            })?
    };

    // Prepare temporary restore location
    let temp_dir = TempDir::new().context("Failed to create temporary directory for restore")?;

    let restic = ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    );

    let progress = restic
        .restore(&snapshot_id, Some(&dependency.source_path), temp_dir.path())
        .with_context(|| {
            format!(
                "Failed to restore dependency '{}:{}' from restic",
                game_name, dependency.id
            )
        })?;

    let restored_source = resolved_restored_source(temp_dir.path(), &source_path)?;

    match dependency.kind {
        DependencyKind::Directory => restore_directory(&restored_source, target_path)?,
        DependencyKind::File => restore_file(&restored_source, target_path)?,
    }

    Ok(DependencyRestoreResult {
        snapshot_id,
        progress,
    })
}

fn resolved_restored_source(temp_root: &Path, source_path: &Path) -> Result<PathBuf> {
    let mut relative = PathBuf::new();
    for component in source_path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => continue,
            Component::CurDir => continue,
            Component::ParentDir => relative.push(".."),
            Component::Normal(part) => relative.push(part),
        }
    }

    if relative.as_os_str().is_empty() {
        return Err(anyhow!(
            "Unable to determine restored path for dependency source: {}",
            source_path.display()
        ));
    }

    let candidate = temp_root.join(&relative);
    if !candidate.exists() {
        return Err(anyhow!(
            "Restored dependency content missing for path: {}",
            candidate.display()
        ));
    }

    Ok(candidate)
}

fn restore_directory(restored_source: &Path, target_path: &Path) -> Result<()> {
    if target_path.exists() {
        fs::remove_dir_all(target_path).with_context(|| {
            format!(
                "Failed to remove existing dependency directory: {}",
                target_path.display()
            )
        })?;
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to ensure parent directory exists: {}",
                parent.display()
            )
        })?;
    }

    if let Err(err) = fs::rename(restored_source, target_path) {
        copy_directory_recursive(restored_source, target_path).with_context(|| {
            format!(
                "Failed to move restored dependency directory to {} (rename error: {})",
                target_path.display(),
                err
            )
        })?;
    }
    Ok(())
}

fn restore_file(restored_source: &Path, target_path: &Path) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to ensure parent directory exists: {}",
                parent.display()
            )
        })?;
    }

    if target_path.exists() {
        fs::remove_file(target_path).with_context(|| {
            format!(
                "Failed to remove existing dependency file: {}",
                target_path.display()
            )
        })?;
    }

    if let Err(err) = fs::rename(restored_source, target_path) {
        fs::copy(restored_source, target_path).with_context(|| {
            format!(
                "Failed to move restored dependency file to {} (rename error: {})",
                target_path.display(),
                err
            )
        })?;
    }
    Ok(())
}

fn copy_directory_recursive(source: &Path, target: &Path) -> Result<()> {
    if !source.is_dir() {
        return Err(anyhow!(
            "Cannot copy dependency directory; source is not a directory: {}",
            source.display()
        ));
    }

    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source).with_context(|| {
            format!(
                "Failed to determine relative path while copying dependency: {}",
                entry.path().display()
            )
        })?;
        let destination = target.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination).with_context(|| {
                format!("Failed to create directory: {}", destination.display())
            })?;
        } else if entry.file_type().is_symlink() {
            let link_target = fs::read_link(entry.path()).with_context(|| {
                format!(
                    "Failed to read symlink while copying dependency: {}",
                    entry.path().display()
                )
            })?;
            create_symlink(&link_target, &destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create parent directory while copying dependency: {}",
                        parent.display()
                    )
                })?;
            }
            fs::copy(entry.path(), &destination).with_context(|| {
                format!(
                    "Failed to copy dependency file to {}",
                    destination.display()
                )
            })?;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    symlink(target, link).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            link.display(),
            target.display()
        )
    })?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    if target.is_dir() {
        symlink_dir(target, link).with_context(|| {
            format!(
                "Failed to create directory symlink {} -> {}",
                link.display(),
                target.display()
            )
        })?;
    } else {
        symlink_file(target, link).with_context(|| {
            format!(
                "Failed to create file symlink {} -> {}",
                link.display(),
                target.display()
            )
        })?;
    }

    Ok(())
}
