use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use walkdir::WalkDir;

use super::types::{EntryKind, PassEntry};
use crate::frecency::FrecencyStore;

pub(super) fn ensure_password_store_dir() -> Result<PathBuf> {
    let dir = password_store_dir()?;
    if !dir.exists() {
        bail!(
            "No password store found at {}. Run `pass init <gpg-id>` first.",
            dir.display()
        );
    }
    Ok(dir)
}

pub(super) fn password_store_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("PASSWORD_STORE_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("Failed to determine home directory"))?;
    Ok(home.join(".password-store"))
}

pub(super) fn load_entries(store_dir: &Path) -> Result<Vec<PassEntry>> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(store_dir).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("Warning: failed to read password-store entry: {err}");
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gpg") {
            continue;
        }

        let relative = path
            .strip_prefix(store_dir)
            .with_context(|| format!("Failed to strip store prefix from {}", path.display()))?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        let without_ext = relative.strip_suffix(".gpg").unwrap_or(&relative);
        let key = without_ext.to_string();
        let kind = if key.ends_with(".otp") {
            EntryKind::Otp
        } else if key.ends_with(".file") {
            EntryKind::File
        } else {
            EntryKind::Password
        };

        entries.push(PassEntry::new(key, path.to_path_buf(), kind));
    }

    entries.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    Ok(entries)
}

pub(super) fn frecency_scores(entries: &[PassEntry]) -> Result<HashMap<String, f64>> {
    let path = frecency_store_path()?;
    let store = FrecencyStore::load(&path);

    Ok(entries
        .iter()
        .map(|entry| (entry.display_name.clone(), store.score(&entry.display_name)))
        .collect())
}

pub(super) fn record_frecency(item: &str) -> Result<()> {
    let path = frecency_store_path()?;
    let mut store = FrecencyStore::load(&path);
    store.record(item);
    store
        .save(&path)
        .context("Failed to save pass frecency store")
}

fn frecency_store_path() -> Result<PathBuf> {
    // Use the platform's temp dir as a fallback so Termux (which exposes
    // $PREFIX/tmp via $TMPDIR rather than /tmp) gets a writable location.
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(env!("CARGO_BIN_NAME"));
    fs::create_dir_all(&cache_dir).context("Failed to create cache directory for pass")?;
    Ok(cache_dir.join("pass_frecency_store.json"))
}
