//! Filesystem locks, content baselines, and append-only shared history.
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

use super::config::{InstantGameConfig, OwnedSaves};
use super::ownership::{OwnerId, SaveOwner, normalized_path};
use super::restic::cache;
use crate::restic::wrapper::Snapshot;

#[derive(Debug, thiserror::Error)]
#[error(
    "Concurrent save uploads created separate versions; retry synchronization to apply the newest saves"
)]
pub struct ConcurrentSaveVersions;

pub struct SaveLock(File);
impl SaveLock {
    pub fn owner(id: &OwnerId) -> Result<Self> {
        let key = format!("{id:?}");
        Self::named(&key)
    }

    pub fn named(key: &str) -> Result<Self> {
        let directory = crate::common::paths::games_config_dir()?.join("locks");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!(
            "{}.lock",
            hex::encode(Sha256::digest(key.as_bytes()))
        ));
        Self::at(&path)
    }

    pub fn at_wait(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        file.lock().context("Waiting for configuration writer")?;
        Ok(Self(file))
    }

    pub fn at(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        file.try_lock().with_context(|| {
            format!(
                "Another save operation holds {}; wait for it to finish",
                path.display()
            )
        })?;
        Ok(Self(file))
    }
}
impl Drop for SaveLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Hash exactly the declared tree, including hidden files, deletions, empty
/// directories and symlink targets. Do not follow links outside the owned tree.
pub fn content_digest(path: &Path) -> Result<String> {
    use std::os::unix::ffi::OsStrExt;
    let mut digest = Sha256::new();
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("missing".into()),
        Err(error) => return Err(error).context("Reading save path"),
        Ok(_) => {}
    }
    let root = normalized_path(path)?;
    let mut entries = walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    for entry in entries {
        let relative = entry.path().strip_prefix(&root)?.as_os_str().as_bytes();
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(relative);
        if entry.file_type().is_symlink() {
            digest.update(b"link");
            let target = std::fs::read_link(entry.path())?;
            ensure!(
                !target.is_absolute(),
                "Absolute save symlink {} is not portable across devices; use a relative link within the owned directory",
                entry.path().display()
            );
            ensure!(
                normalized_path(entry.path())?.starts_with(&root),
                "Save entry {} points outside the owned directory; its contents would not be backed up. Select a path covering the actual save data",
                entry.path().display()
            );
            let bytes = target.as_os_str().as_bytes();
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        } else if entry.file_type().is_dir() {
            digest.update(b"dir");
        } else {
            ensure!(
                entry.file_type().is_file(),
                "Unsupported save entry: {}",
                entry.path().display()
            );
            digest.update(b"file");
            let mut file = File::open(entry.path())?;
            let length = file.metadata()?.len();
            digest.update(length.to_le_bytes());
            let mut buffer = [0u8; 65536];
            let mut read = 0;
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
                read += count as u64;
            }
            ensure!(
                read == length,
                "Save changed while being read: {}",
                entry.path().display()
            );
        }
    }
    Ok(hex::encode(digest.finalize()))
}

/// A newly bound empty directory/file is not a competing save version. Once a
/// baseline exists, an empty tree can represent an intentional deletion.
pub fn has_save_data(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    for entry in walkdir::WalkDir::new(normalized_path(path)?).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.metadata()?.len() > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn latest_save_time(path: &Path) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut latest = None;
    for entry in walkdir::WalkDir::new(normalized_path(path)?).follow_links(false) {
        let entry = entry?;
        let modified: chrono::DateTime<chrono::Utc> =
            std::fs::symlink_metadata(entry.path())?.modified()?.into();
        latest = Some(
            latest.map_or(modified, |previous: chrono::DateTime<chrono::Utc>| {
                previous.max(modified)
            }),
        );
    }
    Ok(latest)
}

pub fn snapshot_save_time(snapshot: &Snapshot) -> Result<chrono::DateTime<chrono::Utc>> {
    let time = snapshot
        .tags
        .iter()
        .find_map(|tag| tag.strip_prefix("instantgame-save-time:"))
        .unwrap_or(&snapshot.time);
    Ok(chrono::DateTime::parse_from_rfc3339(time)?.with_timezone(&chrono::Utc))
}

pub fn snapshot_digest(snapshot: &Snapshot) -> Option<&str> {
    snapshot
        .tags
        .iter()
        .find_map(|tag| tag.strip_prefix("instantgame-content:"))
}

/// A metadata-only history transition is not a remote save edit. In particular,
/// compaction must not turn an offline local edit into a content conflict.
pub fn snapshot_matches_baseline(saves: &OwnedSaves, snapshot: &Snapshot) -> bool {
    saves
        .baseline
        .as_ref()
        .is_some_and(|baseline| snapshot_digest(snapshot) == Some(baseline.content.as_str()))
}

#[derive(Debug, PartialEq, Eq)]
pub enum BaselineAction {
    Skip,
    Backup,
    Restore(String),
    Reconcile,
}

pub fn baseline_action(saves: &OwnedSaves, local: &str, head: Option<&str>) -> BaselineAction {
    let Some(baseline) = &saves.baseline else {
        return match (head, local == "missing") {
            (None, false) => BaselineAction::Backup,
            (Some(head), true) => BaselineAction::Restore(head.into()),
            _ => BaselineAction::Reconcile,
        };
    };
    let local_changed = baseline.content != local;
    let remote_changed = baseline.remote_head.as_deref() != head;
    match (local_changed, remote_changed) {
        (false, false) => BaselineAction::Skip,
        (true, false) if local != "missing" => BaselineAction::Backup,
        (false, true) if head.is_some() => BaselineAction::Restore(head.unwrap_or_default().into()),
        _ => BaselineAction::Reconcile,
    }
}

const PARENT_PREFIX: &str = "instantgame-parent:";
pub fn parent_tags(parents: &[Snapshot]) -> Vec<String> {
    if parents.is_empty() {
        return vec![format!("{PARENT_PREFIX}root")];
    }
    parents
        .iter()
        .map(|snapshot| format!("{PARENT_PREFIX}{}", snapshot.id))
        .collect()
}

/// Every shared snapshot declares all heads it supersedes. Concurrent uploads
/// create separate tips, which must be reconciled before pruning.
pub fn history_heads(snapshots: &[Snapshot]) -> Result<Vec<Snapshot>> {
    let legacy: Vec<_> = snapshots
        .iter()
        .filter(|snapshot| {
            !snapshot
                .tags
                .iter()
                .any(|tag| tag.starts_with(PARENT_PREFIX))
                && snapshot.tags.iter().any(|tag| tag == "instantgame")
        })
        .collect();
    let latest_legacy = legacy
        .iter()
        .map(|snapshot| {
            Ok((
                chrono::DateTime::parse_from_rfc3339(&snapshot.time)?,
                snapshot.id.as_str(),
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .map(|(_, id)| id);
    let mut superseded = std::collections::HashSet::new();
    for snapshot in snapshots {
        let parents: Vec<_> = snapshot
            .tags
            .iter()
            .filter_map(|tag| tag.strip_prefix(PARENT_PREFIX))
            .collect();
        if parents.is_empty() && snapshot.tags.iter().any(|tag| tag == "instantgame") {
            if Some(snapshot.id.as_str()) != latest_legacy {
                superseded.insert(snapshot.id.clone());
            }
            continue;
        }
        ensure!(
            !parents.is_empty(),
            "Shared history contains an untracked snapshot {}; reconcile explicitly",
            snapshot.id
        );
        for parent in parents {
            if parent != "root" {
                superseded.insert(parent.to_string());
            }
        }
    }
    Ok(snapshots
        .iter()
        .filter(|snapshot| !superseded.contains(&snapshot.id))
        .cloned()
        .collect())
}

pub fn fresh_snapshots(config: &InstantGameConfig, owner: &OwnerId) -> Result<Vec<Snapshot>> {
    cache::invalidate_snapshot_cache();
    cache::get_snapshots_for_owner(owner, config)
}

pub fn ensure_ready(owner: &SaveOwner, config: &InstantGameConfig) -> Result<()> {
    if let Some(pending) = &owner.pending_restore {
        bail!(
            "Restore '{pending}' for {} is incomplete; reconcile saves before playing",
            owner.name
        );
    }
    ensure!(
        !owner.needs_repository_reconciliation(&config.repo.as_path().to_string_lossy()),
        "Repository changed for {}; reconcile saves first",
        owner.name
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::game::config::{PathContentKind, SaveBaseline};

    #[test]
    fn content_baseline_detects_bytes_deletions_and_hidden_files_without_timestamps() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("saves");
        std::fs::create_dir(&path)?;
        let first = content_digest(&path)?;
        std::fs::write(path.join(".hidden"), b"progress")?;
        let second = content_digest(&path)?;
        assert_ne!(first, second);
        let metadata = std::fs::metadata(path.join(".hidden"))?;
        std::fs::write(path.join(".hidden"), b"changed!")?;
        File::options()
            .write(true)
            .open(path.join(".hidden"))?
            .set_times(std::fs::FileTimes::new().set_modified(metadata.modified()?))?;
        assert_ne!(content_digest(&path)?, second);
        std::fs::remove_file(path.join(".hidden"))?;
        assert_eq!(content_digest(&path)?, first);
        std::fs::create_dir(path.join("empty"))?;
        assert_ne!(content_digest(&path)?, first);
        assert_eq!(content_digest(&temp.path().join("missing"))?, "missing");
        Ok(())
    }

    #[test]
    fn baseline_distinguishes_one_sided_changes_from_divergence() {
        let mut saves = OwnedSaves::new(TildePath::from_str("/saves"), PathContentKind::Directory);
        saves.baseline = Some(SaveBaseline {
            content: "old".into(),
            remote_head: Some("head".into()),
        });
        assert_eq!(
            baseline_action(&saves, "old", Some("head")),
            BaselineAction::Skip
        );
        assert_eq!(
            baseline_action(&saves, "new", Some("head")),
            BaselineAction::Backup
        );
        assert_eq!(
            baseline_action(&saves, "old", Some("remote")),
            BaselineAction::Restore("remote".into())
        );
        assert_eq!(
            baseline_action(&saves, "new", Some("remote")),
            BaselineAction::Reconcile
        );
        assert_eq!(
            baseline_action(&saves, "missing", Some("head")),
            BaselineAction::Reconcile
        );
        assert_eq!(
            baseline_action(&saves, "old", None),
            BaselineAction::Reconcile
        );
    }

    fn snapshot(id: &str, parents: &[&str]) -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "id": id, "short_id": id, "time": "2026-01-01T00:00:00Z", "tree": "tree", "paths": ["/saves"], "hostname": "device", "username": "user",
            "tags": parents.iter().map(|parent| format!("{PARENT_PREFIX}{parent}")).collect::<Vec<_>>()
        })).unwrap()
    }

    #[test]
    fn concurrent_uploads_preserve_both_branches_until_explicit_resolution() -> Result<()> {
        let root = snapshot("a", &["root"]);
        let left = snapshot("b", &["a"]);
        let right = snapshot("c", &["a"]);
        let mut history = vec![root, left, right];
        assert_eq!(history_heads(&history)?.len(), 2);
        history.push(snapshot("resolved", &["b", "c"]));
        assert_eq!(
            history_heads(&history)?
                .iter()
                .map(|snapshot| snapshot.id.as_str())
                .collect::<Vec<_>>(),
            ["resolved"]
        );
        assert!(history_heads(&[snapshot("untracked", &[])]).is_err());
        Ok(())
    }

    #[test]
    fn private_legacy_history_has_one_anchor_and_new_uploads_can_branch_from_it() -> Result<()> {
        let mut old_a = snapshot("legacy-a", &[]);
        old_a.tags.push("instantgame".into());
        old_a.time = "2026-01-01T00:00:00+02:00".into();
        let mut old_b = snapshot("legacy-b", &[]);
        old_b.tags.push("instantgame".into());
        old_b.time = "2025-12-31T23:00:00Z".into();
        let mut history = vec![old_a, old_b];
        assert_eq!(
            history_heads(&history)?
                .iter()
                .map(|snapshot| snapshot.id.as_str())
                .collect::<Vec<_>>(),
            ["legacy-b"]
        );
        history.push(snapshot("new-a", &["legacy-b"]));
        history.push(snapshot("new-b", &["legacy-b"]));
        assert_eq!(history_heads(&history)?.len(), 2);
        Ok(())
    }

    #[test]
    fn rejects_save_links_outside_the_declared_owner() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("saves");
        std::fs::create_dir(&root)?;
        std::fs::write(root.join("card"), b"progress")?;
        std::os::unix::fs::symlink("card", root.join("inside"))?;
        content_digest(&root)?;
        std::fs::write(temp.path().join("outside"), b"uncovered")?;
        std::os::unix::fs::symlink(temp.path().join("outside"), root.join("outside"))?;
        assert!(content_digest(&root).is_err());
        Ok(())
    }

    #[test]
    fn kernel_lock_excludes_second_operation_and_releases_on_drop() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("owner.lock");
        let first = SaveLock::at(&path)?;
        assert!(SaveLock::at(&path).is_err());
        drop(first);
        assert!(SaveLock::at(&path).is_ok());
        Ok(())
    }
}
