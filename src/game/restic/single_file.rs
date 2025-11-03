use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use walkdir::WalkDir;

use crate::restic::wrapper::{ResticWrapper, RestoreProgress, SnapshotNode};

/// Resolve the path of a single file within a snapshot.
///
/// The caller may provide multiple candidate paths that could match the stored path in the
/// snapshot. Each candidate is compared against the snapshot entries, first by exact match and
/// then via suffix matching to account for cross-device differences (e.g., different home
/// directory prefixes).
pub(crate) fn resolve_snapshot_file_path(
    restic: &ResticWrapper,
    snapshot_id: &str,
    candidate_paths: &[String],
    reference_path: Option<&Path>,
) -> Result<String> {
    let nodes: Vec<SnapshotNode> = restic
        .list_snapshot_nodes(snapshot_id)
        .context("Failed to inspect snapshot contents")?;

    let mut candidates: Vec<String> = candidate_paths.to_vec();

    if let Some(reference) = reference_path {
        let reference_string = reference.to_string_lossy().into_owned();
        if !candidates
            .iter()
            .any(|existing| existing == &reference_string)
        {
            candidates.push(reference_string);
        }
    }

    let file_nodes: Vec<&SnapshotNode> = nodes
        .iter()
        .filter(|node| node.node_type == "file")
        .collect();

    for candidate in &candidates {
        if file_nodes.iter().any(|node| node.path == *candidate) {
            return Ok(candidate.clone());
        }
    }

    let mut best: Option<(usize, &str)> = None;

    for node in file_nodes {
        let node_path = node.path.as_str();
        for candidate in &candidates {
            let score = component_suffix_match(node_path, candidate);
            if score == 0 {
                continue;
            }

            match &mut best {
                Some((best_score, best_path)) => {
                    if score > *best_score
                        || (score == *best_score && node_path.len() > best_path.len())
                    {
                        *best_score = score;
                        *best_path = node_path;
                    }
                }
                None => best = Some((score, node_path)),
            }
        }
    }

    if let Some((_, matched)) = best {
        return Ok(matched.to_string());
    }

    let reference_display = reference_path
        .map(|path| path.display().to_string())
        .or_else(|| candidates.first().cloned())
        .unwrap_or_else(|| "<unknown>".to_string());

    Err(anyhow!(
        "Snapshot '{}' does not contain a file matching '{}'",
        snapshot_id,
        reference_display
    ))
}

/// Restore a single snapshot file into a temporary directory and return the restored path plus
/// restic progress metadata.
pub(crate) fn restore_single_file_into_temp(
    restic: &ResticWrapper,
    snapshot_id: &str,
    snapshot_path: &str,
    temp_restore: &Path,
    expected_path: &Path,
) -> Result<(PathBuf, RestoreProgress)> {
    let progress = restic
        .restore_single_file(snapshot_id, snapshot_path, temp_restore)
        .with_context(|| {
            format!("Failed to restore single file '{snapshot_path}' from snapshot '{snapshot_id}'")
        })?;

    let restored_file = find_restored_file(temp_restore, expected_path).map_err(|err| {
        anyhow!(
            "restic restored '{snapshot_path}' but no files were written to {}: {err}",
            temp_restore.display()
        )
    })?;

    Ok((restored_file, progress))
}

/// Find the file that restic restored within a temporary directory, attempting to match by file
/// name and falling back to the shallowest restored file if needed.
pub(crate) fn find_restored_file(temp_dir: &Path, original_path: &Path) -> Result<PathBuf> {
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

/// Format a human-readable summary from restic restore progress.
pub(crate) fn summarize_restore(progress: &RestoreProgress) -> Option<String> {
    progress.summary.as_ref().map(|summary| {
        format!(
            "restored {} file{}",
            summary.files_restored,
            if summary.files_restored == 1 { "" } else { "s" }
        )
    })
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
