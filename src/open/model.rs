use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::applications::app_info::{ApplicationInfo, ApplicationInfoCache};
use crate::applications::mime_cache::{
    MimeDatabase, canonical_mime_type, get_apps_for_mime, load_mime_database,
};
use crate::applications::system::query_default_app;
use crate::frecency::FrecencyStore;

pub(super) struct Candidate {
    pub app: ApplicationInfo,
    pub recommended: bool,
    pub score: f64,
}

pub(super) struct DiscoveryBatch {
    pub mime: String,
    pub apps: Vec<Candidate>,
    pub warning: Option<String>,
}

pub(super) enum Choice {
    Desktop { app: ApplicationInfo, mime: String },
    Executable(PathBuf),
    RunFile,
}

fn history_path() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| dirs::cache_dir().unwrap_or_else(std::env::temp_dir))
        .join("ins/open-frecency.json")
}

fn history_key(mime: &str, desktop_id: &str) -> String {
    format!("{mime}\n{desktop_id}")
}

pub(super) fn record(mime: &str, desktop_id: &str) -> Result<()> {
    let path = history_path();
    let mut store = FrecencyStore::load(&path);
    store.record(&history_key(mime, desktop_id));
    store.save(&path)
}

pub(super) fn detect_mime(path: &Path) -> Result<String> {
    let mime = if path.is_dir() {
        "inode/directory".to_owned()
    } else {
        let output = Command::new("xdg-mime")
            .args(["query", "filetype"])
            .arg(path)
            .output()
            .context("Failed to detect file type; install xdg-utils")?;
        if !output.status.success() {
            bail!(
                "MIME detection failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let mime = String::from_utf8(output.stdout).context("Invalid MIME detection output")?;
        let mime = mime.trim().to_owned();
        if !mime.contains('/') || mime.chars().any(char::is_whitespace) {
            bail!("MIME detection returned an invalid type");
        }
        mime
    };
    Ok(canonical_mime_type(&mime))
}

/// Emit independent phases so the useful choices never wait for the full catalog.
/// Returning false from the receiver stops further work when the menu has closed.
pub(super) fn discover(mime: String, mut emit: impl FnMut(DiscoveryBatch) -> bool) -> Result<()> {
    let (default, warning) = match query_default_app(&mime) {
        Ok(value) => (value, None),
        Err(error) => (None, Some(format!("Could not query default: {error}"))),
    };
    let history = FrecencyStore::load(&history_path());
    let mut catalog = ApplicationInfoCache::default();
    let mut emitted = HashSet::new();
    let mut apps = Vec::new();
    if let Some(id) = default.filter(|id| id != "ins-open.desktop") {
        let mut app = catalog.get(&id);
        if app.path.is_some() {
            app.is_default = true;
            let score = history.score(&history_key(&mime, &id));
            emitted.insert(id);
            apps.push(Candidate {
                app,
                recommended: true,
                score,
            });
        }
    }
    if !emit(DiscoveryBatch {
        mime: mime.clone(),
        apps,
        warning,
    }) {
        return Ok(());
    }

    let mut database = load_mime_database()?;
    discover_remaining(
        &mime,
        &mut catalog,
        &mut database,
        &history,
        &mut emitted,
        emit,
    );
    Ok(())
}

fn discover_remaining(
    mime: &str,
    catalog: &mut ApplicationInfoCache,
    database: &mut MimeDatabase,
    history: &FrecencyStore,
    emitted: &mut HashSet<String>,
    mut emit: impl FnMut(DiscoveryBatch) -> bool,
) {
    let prefix = format!("{mime}\n");
    let mut known = get_apps_for_mime(mime, database)
        .into_iter()
        .collect::<HashSet<_>>();
    known.extend(
        history
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned)),
    );
    let mut recommended = Vec::new();
    for id in &known {
        if id == "ins-open.desktop" || emitted.contains(id) {
            continue;
        }
        let app = catalog.get(id);
        if app.path.is_some() {
            recommended.push(Candidate {
                score: history.score(&history_key(mime, id)),
                app,
                recommended: true,
            });
            emitted.insert(id.clone());
        }
    }
    rank(&mut recommended);
    if !emit(DiscoveryBatch {
        mime: mime.to_owned(),
        apps: recommended,
        warning: None,
    }) {
        return;
    }

    // Recover handlers absent from mimeinfo.cache as part of the last phase.
    // Already-resolved entries are reused, so every desktop file is parsed once.
    let applications = catalog.all();
    for app in &applications {
        database.add_application(&app.desktop_id, &app.mime_types);
    }
    let supported = get_apps_for_mime(mime, database)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut remaining = applications
        .into_iter()
        .filter(|app| app.desktop_id != "ins-open.desktop" && !emitted.contains(&app.desktop_id))
        .map(|app| Candidate {
            recommended: supported.contains(&app.desktop_id),
            score: 0.0,
            app,
        })
        .collect::<Vec<_>>();
    rank(&mut remaining);
    emit(DiscoveryBatch {
        mime: mime.to_owned(),
        apps: remaining,
        warning: None,
    });
}

fn rank(apps: &mut [Candidate]) {
    apps.sort_by(|a, b| {
        b.recommended
            .cmp(&a.recommended)
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| b.app.is_default.cmp(&a.app.is_default))
            .then_with(|| {
                a.app
                    .name
                    .as_deref()
                    .unwrap_or(&a.app.desktop_id)
                    .to_lowercase()
                    .cmp(
                        &b.app
                            .name
                            .as_deref()
                            .unwrap_or(&b.app.desktop_id)
                            .to_lowercase(),
                    )
            })
            .then_with(|| a.app.desktop_id.cmp(&b.app.desktop_id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendations_arrive_before_catalog_scan_with_history_and_cache_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let entry = "[Desktop Entry]\nType=Application\nName=App\nExec=app %f\n";
        for id in ["indexed", "recent", "other-mime", "unindexed", "ins-open"] {
            let mime = if id == "unindexed" {
                "MimeType=text/plain;\n"
            } else {
                ""
            };
            std::fs::write(
                directory.path().join(format!("{id}.desktop")),
                format!("{entry}{mime}"),
            )
            .unwrap();
        }
        let mut catalog = ApplicationInfoCache::new(vec![directory.path().to_owned()]);
        let mut database = MimeDatabase::default();
        database.add_application("indexed.desktop", &["text/plain".into()]);
        database.add_application("removed.desktop", &["text/plain".into()]);
        let mut history = FrecencyStore::new();
        history.record(&history_key("text/plain", "recent.desktop"));
        history.record(&history_key("image/png", "other-mime.desktop"));
        let mut batches = Vec::new();
        discover_remaining(
            "text/plain",
            &mut catalog,
            &mut database,
            &history,
            &mut HashSet::new(),
            |snapshot| {
                if batches.is_empty() {
                    assert_eq!(
                        snapshot
                            .apps
                            .iter()
                            .map(|app| app.app.desktop_id.as_str())
                            .collect::<Vec<_>>(),
                        ["recent.desktop", "indexed.desktop"]
                    );
                    // This appears in the final batch only if the catalog has
                    // not been enumerated before delivering recommendations.
                    std::fs::write(directory.path().join("late.desktop"), entry).unwrap();
                }
                batches.push(snapshot);
                true
            },
        );
        assert_eq!(batches.len(), 2);
        assert!(
            batches
                .iter()
                .flat_map(|batch| &batch.apps)
                .all(|candidate| candidate.app.desktop_id != "ins-open.desktop")
        );
        let remaining = &batches[1].apps;
        assert_eq!(remaining[0].app.desktop_id, "unindexed.desktop");
        assert!(remaining[0].recommended);
        assert!(
            remaining
                .iter()
                .any(|app| app.app.desktop_id == "late.desktop")
        );
        assert!(
            remaining
                .iter()
                .find(|app| app.app.desktop_id == "other-mime.desktop")
                .is_some_and(|app| !app.recommended)
        );
    }

    #[test]
    fn closed_receiver_skips_catalog_scan() {
        let directory = tempfile::tempdir().unwrap();
        let mut catalog = ApplicationInfoCache::new(vec![directory.path().to_owned()]);
        discover_remaining(
            "text/plain",
            &mut catalog,
            &mut MimeDatabase::default(),
            &FrecencyStore::new(),
            &mut HashSet::new(),
            |_| false,
        );
        std::fs::write(
            directory.path().join("late.desktop"),
            "[Desktop Entry]\nType=Application\nExec=app\n",
        )
        .unwrap();
        assert_eq!(catalog.all().len(), 1);
    }

    #[test]
    fn usage_is_scoped_to_mime_type() {
        let mut history = FrecencyStore::new();
        history.record(&history_key("image/png", "viewer.desktop"));
        assert!(history.score(&history_key("image/png", "viewer.desktop")) > 0.0);
        assert_eq!(
            history.score(&history_key("text/plain", "viewer.desktop")),
            0.0
        );
    }
}
