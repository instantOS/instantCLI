use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use nix::fcntl::{Flock, FlockArg};

use crate::applications::app_info::{ApplicationInfo, ApplicationInfoCache};
use crate::applications::mime_cache::{
    MimeDatabase, canonical_mime_type, get_apps_for_mime, load_mime_database,
};
use crate::applications::system::query_default_app;
use crate::frecency::FrecencyStore;

#[derive(Clone, Debug)]
pub(super) enum Application {
    Desktop(ApplicationInfo),
    Executable(PathBuf),
}

impl Application {
    pub(super) fn key(&self) -> String {
        match self {
            Self::Desktop(app) => app.desktop_id.clone(),
            Self::Executable(path) => format!("exec:{}", path.display()),
        }
    }

    pub(super) fn name(&self) -> Cow<'_, str> {
        match self {
            Self::Desktop(app) => Cow::Borrowed(app.name.as_deref().unwrap_or(&app.desktop_id)),
            Self::Executable(path) => path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy(),
        }
    }

    pub(super) fn is_default(&self) -> bool {
        matches!(self, Self::Desktop(app) if app.is_default)
    }
}

pub(super) struct Candidate {
    pub app: Application,
    pub recommended: bool,
    pub score: f64,
}

pub(super) struct DiscoveryBatch {
    pub mime: String,
    pub apps: Vec<Candidate>,
    pub warning: Option<String>,
}

pub(super) enum Choice {
    Desktop {
        app: ApplicationInfo,
        mime: String,
    },
    Executable {
        executable: PathBuf,
        mime: Option<String>,
    },
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

// Serialize read/modify/write operations across simultaneous picker processes.
fn update_history(
    path: &Path,
    update: impl FnOnce(&mut FrecencyStore) -> bool,
) -> Result<FrecencyStore> {
    let parent = path.parent().context("History path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))
        .context("Cannot open application history lock")?;
    let _lock = Flock::lock(file, FlockArg::LockExclusive)
        .map_err(|(_, error)| error)
        .context("Cannot lock application history")?;
    let mut history = FrecencyStore::load(path);
    if update(&mut history) {
        history.save(path)?;
    }
    Ok(history)
}

pub(super) fn record(mime: &str, desktop_id: &str) -> Result<()> {
    update_history(&history_path(), |history| {
        history.record(&history_key(mime, desktop_id));
        true
    })?;
    Ok(())
}

const CUSTOM_PREFIX: &str = "exec:";
const CUSTOM_MAX_PER_MIME: usize = 20;
const CUSTOM_MAX_TOTAL: usize = 100;
const CUSTOM_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

fn custom_entry(key: &str) -> Option<(&str, &Path)> {
    let (mime, id) = key.split_once('\n')?;
    let path = Path::new(id.strip_prefix(CUSTOM_PREFIX)?);
    Some((mime, path))
}

fn prune_custom(
    history: &mut FrecencyStore,
    now: f64,
    mut available: impl FnMut(&Path) -> bool,
) -> bool {
    let mut keys: Vec<_> = history
        .keys()
        .filter(|key| custom_entry(key).is_some())
        .map(|key| {
            (
                key.to_owned(),
                history.last_accessed(key).unwrap_or_default(),
            )
        })
        .collect();
    keys.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut per_mime = HashMap::new();
    let mut total = 0;
    let mut changed = false;
    for (key, accessed) in keys {
        let Some((mime, path)) = custom_entry(&key) else {
            continue;
        };
        let count = per_mime.entry(mime.to_owned()).or_insert(0);
        if !accessed.is_finite()
            || now - accessed > CUSTOM_MAX_AGE.as_secs_f64()
            || total >= CUSTOM_MAX_TOTAL
            || *count >= CUSTOM_MAX_PER_MIME
            || !path.is_absolute()
            || !available(path)
        {
            history.remove(&key);
            changed = true;
        } else {
            *count += 1;
            total += 1;
        }
    }
    changed
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub(super) fn record_custom(mime: &str, executable: &Path) -> Result<()> {
    let executable = executable
        .to_str()
        .context("Cannot remember a non-UTF-8 executable path")?;
    update_history(&history_path(), |history| {
        history.record(&history_key(mime, &format!("{CUSTOM_PREFIX}{executable}")));
        prune_custom(history, now(), |path| which::which(path).is_ok());
        true
    })?;
    Ok(())
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
    let mut history = FrecencyStore::load(&history_path());
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
                app: Application::Desktop(app),
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

    if history.keys().any(|key| custom_entry(key).is_some()) {
        match update_history(&history_path(), |history| {
            prune_custom(history, now(), |path| which::which(path).is_ok())
        }) {
            Ok(updated) => history = updated,
            Err(error) => {
                prune_custom(&mut history, now(), |path| which::which(path).is_ok());
                eprintln!("Warning: could not prune remembered executables: {error:#}");
            }
        }
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
        if let Some(path) = id.strip_prefix(CUSTOM_PREFIX) {
            recommended.push(Candidate {
                score: history.score(&history_key(mime, id)),
                app: Application::Executable(PathBuf::from(path)),
                recommended: true,
            });
            emitted.insert(id.clone());
            continue;
        }
        let app = catalog.get(id);
        if app.path.is_some() {
            recommended.push(Candidate {
                score: history.score(&history_key(mime, id)),
                app: Application::Desktop(app),
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
            app: Application::Desktop(app),
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
            .then_with(|| b.app.is_default().cmp(&a.app.is_default()))
            .then_with(|| {
                a.app
                    .name()
                    .to_lowercase()
                    .cmp(&b.app.name().to_lowercase())
            })
            .then_with(|| a.app.key().cmp(&b.app.key()))
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
        history.record(&history_key("text/plain", "exec:/bin/custom-one"));
        history.record(&history_key("text/plain", "exec:/bin/custom-one"));
        history.record(&history_key("image/png", "exec:/bin/image-only"));
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
                            .map(|app| app.app.key())
                            .collect::<Vec<_>>(),
                        ["exec:/bin/custom-one", "recent.desktop", "indexed.desktop"]
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
                .all(|candidate| candidate.app.key() != "ins-open.desktop")
        );
        let remaining = &batches[1].apps;
        assert_eq!(remaining[0].app.key(), "unindexed.desktop");
        assert!(remaining[0].recommended);
        assert!(remaining.iter().any(|app| app.app.key() == "late.desktop"));
        assert!(
            remaining
                .iter()
                .find(|app| app.app.key() == "other-mime.desktop")
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
    fn custom_cache_is_bounded_and_keeps_recent_entries_per_mime() -> Result<()> {
        let mut history = FrecencyStore::new();
        history.record("text/plain\neditor.desktop");
        for index in 0..150 {
            history.record(&format!(
                "type/{}\nexec:/bin/custom-{index}",
                if index >= 100 { 6 } else { index % 6 }
            ));
        }
        // Give every entry a deterministic recency independent of machine speed.
        let mut json = serde_json::to_value(&history)?;
        for (key, entry) in json["entries"].as_object_mut().context("Missing entries")? {
            let index = key
                .rsplit('-')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or_default();
            entry["last_accessed"] = serde_json::json!(1000 + index);
        }
        let mut history: FrecencyStore = serde_json::from_value(json)?;
        assert!(prune_custom(&mut history, 1200.0, |_| true));
        assert_eq!(
            history
                .keys()
                .filter(|key| custom_entry(key).is_some())
                .count(),
            CUSTOM_MAX_TOTAL
        );
        for mime in 0..7 {
            assert!(
                history
                    .keys()
                    .filter(|key| key.starts_with(&format!("type/{mime}\n")))
                    .count()
                    <= CUSTOM_MAX_PER_MIME
            );
        }
        assert!(history.keys().any(|key| key.ends_with("custom-149")));
        assert!(!history.keys().any(|key| key.ends_with("custom-0")));
        assert!(
            history
                .keys()
                .any(|key| key == "text/plain\neditor.desktop")
        );
        assert!(!prune_custom(&mut history, 1200.0, |_| true));
        Ok(())
    }

    #[test]
    fn custom_cache_expires_and_removes_unavailable_or_relative_paths() {
        let mut history = FrecencyStore::new();
        for id in [
            "exec:/valid",
            "exec:/missing",
            "exec:relative",
            "editor.desktop",
        ] {
            history.record(&history_key("text/plain", id));
        }
        let time = now();
        assert!(prune_custom(&mut history, time, |path| path != Path::new("/missing")));
        assert_eq!(history.keys().count(), 2);
        assert!(prune_custom(
            &mut history,
            time + CUSTOM_MAX_AGE.as_secs_f64() + 1.0,
            |_| true
        ));
        assert_eq!(
            history.keys().collect::<Vec<_>>(),
            ["text/plain\neditor.desktop"]
        );
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
