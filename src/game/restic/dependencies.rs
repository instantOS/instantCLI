use std::fs;
use std::path::Path;

use crate::game::config::{GameDependency, InstantGameConfig};
use crate::game::restic::{cache, tags};
use crate::restic::wrapper::{BackupProgress, ResticWrapper, Snapshot};
use anyhow::{Context, Result, anyhow};

/// Result of creating or reusing a dependency snapshot
pub struct DependencyBackupResult {
    pub snapshot_id: String,
    pub reused_existing: bool,
    pub progress: BackupProgress,
}

/// Result of restoring a dependency snapshot
pub struct DependencyRestoreOutcome {
    pub snapshot_id: String,
    pub summary: Option<String>,
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

/// Restore dependency snapshot content into target path (directories only)
pub fn restore_dependency(
    game_name: &str,
    dependency: &GameDependency,
    config: &InstantGameConfig,
    install_path: &Path,
) -> Result<DependencyRestoreOutcome> {
    let snapshot_id = list_dependency_snapshots(game_name, &dependency.id, config)?
        .first()
        .map(|snapshot| snapshot.id.clone())
        .ok_or_else(|| {
            anyhow!(
                "No snapshot found for dependency '{}:{}'",
                game_name,
                dependency.id
            )
        })?;

    let restic = ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    );

    fs::create_dir_all(install_path).with_context(|| {
        format!(
            "Failed to create dependency install directory: {}",
            install_path.display()
        )
    })?;

    let progress = restic
        .restore(&snapshot_id, Some(&dependency.source_path), install_path)
        .with_context(|| {
            format!(
                "Failed to restore dependency '{}:{}' from restic",
                game_name, dependency.id
            )
        })?;

    if !fs::metadata(install_path)
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "Dependency '{}' ({}) did not restore as a directory; file dependencies are not supported",
            dependency.id,
            dependency.source_path
        ));
    }

    let summary = Some(
        progress
            .summary
            .as_ref()
            .map(|restored| format!("restored {} files", restored.files_restored))
            .unwrap_or_else(|| "restore completed".to_string()),
    );

    Ok(DependencyRestoreOutcome {
        snapshot_id,
        summary,
    })
}
