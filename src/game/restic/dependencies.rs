use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::game::config::{GameDependency, InstantGameConfig, PathContentKind};
use crate::game::restic::{cache, tags};
use crate::restic::wrapper::{
    BackupProgress, ResticWrapper, RestoreProgress, Snapshot, SnapshotNode,
};
use anyhow::{Context, Result, anyhow};
use walkdir::WalkDir;

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

    let metadata = fs::metadata(source_path).with_context(|| {
        format!(
            "Failed to read metadata for dependency source path: {}",
            source_path.display()
        )
    })?;
    let source_kind: PathContentKind = metadata.into();

    let restic = ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    );

    let tags = tags::create_dependency_tags(game_name, dependency_id);
    let mut include_filter = None;
    let mut backup_paths: Vec<&Path> = vec![source_path];

    if source_kind.is_file() {
        let parent = source_path.parent().ok_or_else(|| {
            anyhow!(
                "Cannot determine directory to snapshot for dependency '{}:{}'",
                game_name,
                dependency_id
            )
        })?;
        backup_paths = vec![parent];
        include_filter = source_path
            .strip_prefix(parent)
            .ok()
            .map(|p| p.to_string_lossy().to_string());
    }

    let progress = restic
        .backup_with_filter(&backup_paths, tags, include_filter.as_deref())
        .with_context(|| {
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

    let source_kind = dependency.source_type;

    let summary = match source_kind {
        PathContentKind::Directory => {
            fs::create_dir_all(install_path).with_context(|| {
                format!(
                    "Failed to create dependency install directory: {}",
                    install_path.display()
                )
            })?;

            let progress = restic
                .restore_with_filter(
                    &snapshot_id,
                    Some(&dependency.source_path),
                    install_path,
                    None,
                )
                .with_context(|| {
                    format!(
                        "Failed to restore dependency '{}:{}' from restic",
                        game_name, dependency.id
                    )
                })?;

            summarize_restore(&progress)
        }
        PathContentKind::File => {
            if let Some(parent) = install_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create parent directory for dependency restore: {}",
                        parent.display()
                    )
                })?;
            }

            let resolved_snapshot_path =
                resolve_dependency_snapshot_file_path(&restic, &snapshot_id, dependency)?;

            let temp_restore = std::env::temp_dir().join(format!(
                "ins-dep-restore-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ));

            fs::create_dir_all(&temp_restore).with_context(|| {
                format!(
                    "Failed to create temporary directory for dependency restore: {}",
                    temp_restore.display()
                )
            })?;

            let progress = restic
                .restore_single_file(&snapshot_id, &resolved_snapshot_path, &temp_restore)
                .with_context(|| {
                    format!(
                        "Failed to restore dependency '{}:{}' file from restic",
                        game_name, dependency.id
                    )
                })?;

            let restored_file =
                find_restored_file(&temp_restore, Path::new(&dependency.source_path))
                    .with_context(|| {
                        format!(
                            "Restic restore for dependency '{}:{}' did not produce any files",
                            game_name, dependency.id
                        )
                    })?;

            fs::copy(&restored_file, install_path).with_context(|| {
                format!(
                    "Failed to copy restored dependency file from {} to {}",
                    restored_file.display(),
                    install_path.display()
                )
            })?;

            let _ = fs::remove_dir_all(&temp_restore);

            summarize_restore(&progress)
        }
    }
    .or_else(|| Some("restore completed".to_string()));

    Ok(DependencyRestoreOutcome {
        snapshot_id,
        summary,
    })
}

fn summarize_restore(progress: &RestoreProgress) -> Option<String> {
    progress.summary.as_ref().map(|summary| {
        format!(
            "restored {} file{}",
            summary.files_restored,
            if summary.files_restored == 1 { "" } else { "s" }
        )
    })
}

fn resolve_dependency_snapshot_file_path(
    restic: &ResticWrapper,
    snapshot_id: &str,
    dependency: &GameDependency,
) -> Result<String> {
    let nodes: Vec<SnapshotNode> = restic
        .list_snapshot_nodes(snapshot_id)
        .context("Failed to inspect snapshot contents for dependency restore")?;

    let mut best: Option<(usize, &str)> = None;

    for node in nodes.iter().filter(|node| node.node_type == "file") {
        if node.path == dependency.source_path {
            return Ok(node.path.clone());
        }

        let score = component_suffix_match(&node.path, &dependency.source_path);
        if score == 0 {
            continue;
        }

        match &mut best {
            Some((best_score, best_path)) => {
                if score > *best_score
                    || (score == *best_score && node.path.len() > (*best_path).len())
                {
                    *best_score = score;
                    *best_path = node.path.as_str();
                }
            }
            None => best = Some((score, node.path.as_str())),
        }
    }

    if let Some((_, path)) = best {
        return Ok(path.to_string());
    }

    Err(anyhow!(
        "Snapshot '{}' does not contain a file matching dependency source path '{}'",
        snapshot_id,
        dependency.source_path
    ))
}

fn find_restored_file(temp_dir: &Path, original_path: &Path) -> Result<std::path::PathBuf> {
    let original_name = original_path.file_name();
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
        _ => candidates
            .into_iter()
            .min_by_key(|path| path.components().count())
            .ok_or_else(|| anyhow!("restic restore produced multiple files unexpectedly")),
    }
}

fn component_suffix_match(candidate: &str, reference: &str) -> usize {
    let candidate_parts: Vec<&str> = candidate.trim_matches('/').split('/').collect();
    let reference_parts: Vec<&str> = reference.trim_matches('/').split('/').collect();

    candidate_parts
        .iter()
        .rev()
        .zip(reference_parts.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
}
