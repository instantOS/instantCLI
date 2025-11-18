use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result};

use crate::game::config::{InstantGameConfig, PathContentKind};
use crate::game::restic::{cache, tags};
use crate::restic::wrapper::{ResticWrapper, Snapshot};

use super::paths::{PathInfo, extract_unique_paths_from_snapshots};

#[derive(Debug, Clone)]
pub(super) struct SnapshotOverview {
    pub snapshot_count: usize,
    pub hosts: BTreeSet<String>,
    pub latest_snapshot_id: Option<String>,
    pub latest_snapshot_time: Option<String>,
    pub latest_snapshot_host: Option<String>,
    pub unique_paths: Vec<PathInfo>,
}

pub(super) fn collect_snapshot_overview(
    game_config: &InstantGameConfig,
) -> Result<HashMap<String, SnapshotOverview>> {
    let snapshots =
        cache::get_repository_snapshots(game_config).context("Failed to list restic snapshots")?;

    let mut grouped: HashMap<String, Vec<Snapshot>> = HashMap::new();

    for snapshot in snapshots {
        if let Some(game_name) = tags::extract_game_name_from_tags(&snapshot.tags) {
            grouped.entry(game_name).or_default().push(snapshot);
        }
    }

    let mut overview = HashMap::new();

    for (name, mut snaps) in grouped {
        snaps.sort_by(|a, b| b.time.cmp(&a.time));

        let unique_paths = extract_unique_paths_from_snapshots(&snaps)?;

        let mut hosts = BTreeSet::new();
        let mut latest_time = None;
        let mut latest_host = None;

        for snapshot in &snaps {
            hosts.insert(snapshot.hostname.clone());

            if latest_time
                .as_ref()
                .map(|time| snapshot.time > *time)
                .unwrap_or(true)
            {
                latest_time = Some(snapshot.time.clone());
                latest_host = Some(snapshot.hostname.clone());
            }
        }

        let entry = SnapshotOverview {
            snapshot_count: snaps.len(),
            hosts,
            latest_snapshot_id: snaps.first().map(|snapshot| snapshot.id.clone()),
            latest_snapshot_time: latest_time,
            latest_snapshot_host: latest_host,
            unique_paths,
        };

        overview.insert(name, entry);
    }

    Ok(overview)
}

pub(super) fn format_snapshot_timestamp(iso: &str, host: Option<&str>) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let local = parsed.with_timezone(&chrono::Local);
    let timestamp = local.format("%Y-%m-%d %H:%M:%S").to_string();

    if let Some(host) = host {
        Some(format!("{timestamp} ({host})"))
    } else {
        Some(timestamp)
    }
}

pub(super) fn infer_snapshot_kind(
    game_config: &InstantGameConfig,
    snapshot_id: &str,
) -> Result<PathContentKind> {
    let file_count = count_snapshot_files(game_config, snapshot_id)?;
    if file_count == 1 {
        Ok(PathContentKind::File)
    } else {
        Ok(PathContentKind::Directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot::path_serde::TildePath;
    use tempfile::TempDir;

    fn ensure_restic_available() -> bool {
        which::which("restic").is_ok()
    }

    fn init_config(temp_dir: &TempDir, password: &str) -> InstantGameConfig {
        let repo_dir = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_dir).expect("failed to create repo dir");

        InstantGameConfig {
            repo: TildePath::new(repo_dir.clone()),
            repo_password: password.to_string(),
            games: Vec::new(),
            retention_policy: Default::default(),
        }
    }

    fn restic_wrapper_for_config(
        config: &InstantGameConfig,
    ) -> anyhow::Result<crate::restic::wrapper::ResticWrapper> {
        Ok(crate::restic::wrapper::ResticWrapper::new(
            config.repo.as_path().to_string_lossy().to_string(),
            config.repo_password.clone(),
        )
        .context("Failed to initialize restic wrapper")?)
    }

    fn latest_snapshot_id(config: &InstantGameConfig) -> String {
        crate::game::restic::cache::invalidate_snapshot_cache();
        crate::game::restic::cache::get_repository_snapshots(config)
            .expect("failed to list snapshots")
            .first()
            .expect("expected snapshot")
            .id
            .clone()
    }

    #[test]
    fn infer_snapshot_kind_detects_single_file_save() -> anyhow::Result<()> {
        if !ensure_restic_available() {
            eprintln!("restic binary not available; skipping test");
            return Ok(());
        }

        let temp_dir = tempfile::tempdir()?;
        let config = init_config(&temp_dir, "testpass-file");
        let restic = restic_wrapper_for_config(&config)?;
        restic.init_repository()?;

        let save_file = temp_dir.path().join("save.sav");
        std::fs::write(&save_file, b"fusion")?;
        restic.backup(&[&save_file], vec!["game:test".to_string()])?;

        let snapshot_id = latest_snapshot_id(&config);
        let kind = infer_snapshot_kind(&config, &snapshot_id)?;
        assert_eq!(kind, PathContentKind::File);

        Ok(())
    }

    #[test]
    fn infer_snapshot_kind_detects_directory_save() -> anyhow::Result<()> {
        if !ensure_restic_available() {
            eprintln!("restic binary not available; skipping test");
            return Ok(());
        }

        let temp_dir = tempfile::tempdir()?;
        let config = init_config(&temp_dir, "testpass-dir");
        let restic = restic_wrapper_for_config(&config)?;
        restic.init_repository()?;

        let save_dir = temp_dir.path().join("SaveData");
        std::fs::create_dir_all(&save_dir)?;
        std::fs::write(save_dir.join("slot1.dat"), b"slot")?;
        std::fs::write(save_dir.join("slot2.dat"), b"slot")?;
        restic.backup(&[&save_dir], vec!["game:test".to_string()])?;

        let snapshot_id = latest_snapshot_id(&config);
        let kind = infer_snapshot_kind(&config, &snapshot_id)?;
        assert_eq!(kind, PathContentKind::Directory);

        Ok(())
    }
}

fn count_snapshot_files(game_config: &InstantGameConfig, snapshot_id: &str) -> Result<usize> {
    let restic = ResticWrapper::new(
        game_config.repo.as_path().to_string_lossy().to_string(),
        game_config.repo_password.clone(),
    )
    .context("Failed to initialize restic wrapper")?;

    let nodes = restic
        .list_snapshot_nodes(snapshot_id)
        .context("Failed to inspect snapshot contents")?;

    Ok(nodes.iter().filter(|node| node.node_type == "file").count())
}
