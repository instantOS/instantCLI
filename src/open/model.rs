use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::applications::app_info::{ApplicationInfo, ApplicationInfoCache};
use crate::applications::mime_cache::{get_apps_for_mime, load_mime_database};
use crate::applications::system::query_default_app;
use crate::frecency::FrecencyStore;

pub(super) struct Candidate {
    pub app: ApplicationInfo,
    pub recommended: bool,
    pub score: f64,
}

pub(super) struct Snapshot {
    pub mime: String,
    pub apps: Vec<Candidate>,
    pub warning: Option<String>,
}

pub(super) struct Choice {
    pub app: ApplicationInfo,
    pub mime: String,
    pub use_default: bool,
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
    let database = load_mime_database()?;
    Ok(database.canonical_type(&mime).to_owned())
}

pub(super) fn discover(mime: String) -> Result<Snapshot> {
    let mut database = load_mime_database()?;
    let (default, warning) = match query_default_app(&mime) {
        Ok(value) => (value, None),
        Err(error) => (None, Some(format!("Could not query default: {error}"))),
    };
    let mut catalog = ApplicationInfoCache::default();
    let applications = catalog.all();
    for app in &applications {
        database.add_application(&app.desktop_id, &app.mime_types);
    }
    let supported = get_apps_for_mime(&mime, &database);
    let history = FrecencyStore::load(&history_path());
    let mut apps = applications
        .into_iter()
        .map(|mut app| {
            app.is_default = default.as_deref() == Some(&app.desktop_id);
            let score = history.score(&history_key(&mime, &app.desktop_id));
            let recommended = app.is_default || supported.contains(&app.desktop_id) || score > 0.0;
            Candidate {
                app,
                recommended,
                score,
            }
        })
        .collect::<Vec<_>>();
    rank(&mut apps);
    Ok(Snapshot {
        mime,
        apps,
        warning,
    })
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
