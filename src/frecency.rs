//! Frecency ("frequency + recency") scoring shared by menus.
//!
//! Each tracked item keeps a score anchored to the time of its last access.
//! Reading a score decays it exponentially based on the configured half-life,
//! so items that were used often but long ago lose to items used recently.
//! The store persists as a small versioned JSON file in the cache directory.
//! Discarded files (corrupt or foreign format) are removed on load so the
//! accompanying warning surfaces exactly once.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

/// On-disk format version. Stores from any other format or version are discarded.
const FORMAT_VERSION: u32 = 1;

/// An item's score halves this often without new accesses.
const HALF_LIFE: Duration = Duration::from_secs(60 * 60 * 24 * 3);

/// Entries untouched for this long are dropped whenever the store is saved.
const PRUNE_AFTER: Duration = Duration::from_secs(60 * 60 * 24 * 30);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    /// Score as of `last_accessed`; decays when evaluated at a later time.
    score: f64,
    num_accesses: u32,
    /// Unix timestamp (seconds) of the most recent access.
    last_accessed: f64,
}

/// A persisted set of frecency scores, keyed by item name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrecencyStore {
    version: u32,
    half_life_secs: f64,
    entries: BTreeMap<String, Entry>,
}

impl Default for FrecencyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FrecencyStore {
    /// Create an empty store with the default half-life.
    pub fn new() -> Self {
        Self {
            version: FORMAT_VERSION,
            half_life_secs: HALF_LIFE.as_secs_f64(),
            entries: BTreeMap::new(),
        }
    }

    /// Load the store at `path`. A missing, corrupt, or foreign-format file
    /// yields a fresh store so a bad cache can never break a menu. Discarded
    /// files are removed so the warning is reported only once.
    pub fn load(path: &Path) -> Self {
        let Ok(content) = fs::read_to_string(path) else {
            return Self::new();
        };

        match serde_json::from_str::<FrecencyStore>(&content) {
            Ok(store) if store.version == FORMAT_VERSION => store,
            Ok(store) => {
                eprintln!(
                    "Warning: discarding frecency store {} with unsupported version {}",
                    path.display(),
                    store.version
                );
                Self::discard(path);
                Self::new()
            }
            Err(error) => {
                eprintln!(
                    "Warning: discarding unreadable frecency store {}: {error}",
                    path.display()
                );
                Self::discard(path);
                Self::new()
            }
        }
    }

    /// Remove a store file that could not be used. Failure is tolerated: the
    /// worst case is the warning repeating on the next load.
    fn discard(path: &Path) {
        if let Err(error) = fs::remove_file(path) {
            eprintln!(
                "Warning: failed to remove discarded frecency store {}: {error}",
                path.display()
            );
        }
    }

    /// Prune stale entries and atomically write the store to `path`.
    pub fn save(&mut self, path: &Path) -> Result<()> {
        self.prune(unix_now());

        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize frecency store")?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .context("Frecency store path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;

        // Write to a sibling temporary file and rename so a crash mid-write
        // cannot leave a truncated store behind.
        let mut temp = NamedTempFile::new_in(parent)
            .context("Failed to create temporary frecency store file")?;
        temp.write_all(json.as_bytes())
            .context("Failed to write temporary frecency store")?;
        temp.persist(path).map_err(|error| {
            anyhow::anyhow!("Failed to save frecency store {}: {error}", path.display())
        })?;

        Ok(())
    }

    /// Record an access of `item` at the current time.
    pub fn record(&mut self, item: &str) {
        self.record_at(item, unix_now());
    }

    /// Return `item`'s score decayed to the current time; untracked items score 0.0.
    pub fn score(&self, item: &str) -> f64 {
        self.score_at(item, unix_now())
    }

    fn record_at(&mut self, item: &str, now: f64) {
        let half_life_secs = self.half_life_secs;
        let entry = self
            .entries
            .entry(item.to_owned())
            .or_insert_with(|| Entry {
                score: 0.0,
                num_accesses: 0,
                last_accessed: now,
            });

        // Decay the anchored score to `now`, then add this access's weight.
        let factor = decay_factor(entry.last_accessed, now, half_life_secs);
        entry.score = entry.score * factor + 1.0;
        entry.last_accessed = now;
        entry.num_accesses += 1;
    }

    fn score_at(&self, item: &str, now: f64) -> f64 {
        match self.entries.get(item) {
            Some(entry) => {
                entry.score * decay_factor(entry.last_accessed, now, self.half_life_secs)
            }
            None => 0.0,
        }
    }

    fn prune(&mut self, now: f64) {
        let cutoff = now - PRUNE_AFTER.as_secs_f64();
        self.entries
            .retain(|_, entry| entry.last_accessed >= cutoff);
    }
}

/// Fraction of a score that remains when decayed from `anchor` to `now`.
fn decay_factor(anchor: f64, now: f64, half_life_secs: f64) -> f64 {
    2.0f64.powf(-(now - anchor) / half_life_secs)
}

/// Current Unix time in seconds; 0.0 if the clock is before the epoch.
fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store_with_half_life(half_life_secs: f64) -> FrecencyStore {
        FrecencyStore {
            version: FORMAT_VERSION,
            half_life_secs,
            entries: BTreeMap::new(),
        }
    }

    #[test]
    fn score_halves_after_one_half_life() {
        let mut store = store_with_half_life(100.0);
        store.record_at("a", 0.0);

        assert!((store.score_at("a", 100.0) - 0.5).abs() < 1e-9);
        assert!((store.score_at("a", 50.0) - 2.0f64.sqrt().recip()).abs() < 1e-9);
        assert!((store.score_at("a", 0.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn repeated_access_outweighs_single_access() {
        let mut store = store_with_half_life(100.0);
        store.record_at("once", 0.0);
        store.record_at("twice", 0.0);
        store.record_at("twice", 0.0);

        assert_eq!(store.entries["twice"].num_accesses, 2);
        assert!(store.score_at("twice", 0.0) > store.score_at("once", 0.0));
    }

    #[test]
    fn recency_can_outweigh_frequency() {
        let mut store = store_with_half_life(100.0);
        store.record_at("frequent_old", 0.0);
        store.record_at("recent_once", 150.0);

        // The older item was accessed twice as often, but 150 seconds ago the
        // recent access still dominates at a half-life of 100 seconds.
        assert!(store.score_at("recent_once", 300.0) > store.score_at("frequent_old", 300.0));
    }

    #[test]
    fn untracked_items_score_zero() {
        let store = store_with_half_life(100.0);

        assert_eq!(store.score("never"), 0.0);
    }

    #[test]
    fn save_and_load_round_trip() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("store.json");
        let mut store = store_with_half_life(100.0);
        store.record_at("a", 10.0);
        store.record_at("b", 20.0);
        store.record_at("b", 20.0);

        store.save(&path)?;
        let loaded = FrecencyStore::load(&path);

        assert_eq!(loaded.version, FORMAT_VERSION);
        assert!((loaded.half_life_secs - 100.0).abs() < 1e-9);
        assert!((loaded.score_at("b", 25.0) - store.score_at("b", 25.0)).abs() < 1e-9);
        Ok(())
    }

    #[test]
    fn foreign_format_starts_fresh() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("store.json");
        // Shape of the old `fre` crate format: no version field.
        fs::write(
            &path,
            r#"{"reference_time": 1.0, "half_life": 259200.0, "items": []}"#,
        )?;

        let loaded = FrecencyStore::load(&path);

        assert_eq!(loaded.entries.len(), 0);
        // The discarded file is removed so the warning is not repeated.
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn corrupt_file_starts_fresh_and_can_be_rewritten() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("store.json");
        fs::write(&path, "not json")?;

        let mut loaded = FrecencyStore::load(&path);
        assert_eq!(loaded.entries.len(), 0);
        assert!(!path.exists());

        loaded.record("recovered");
        loaded.save(&path)?;

        let reloaded = FrecencyStore::load(&path);
        assert_eq!(reloaded.entries.len(), 1);
        Ok(())
    }

    #[test]
    fn save_prunes_stale_entries() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("store.json");
        let now = unix_now();
        let mut store = store_with_half_life(100.0);
        store.record_at("stale", now - PRUNE_AFTER.as_secs_f64() - 1.0);
        store.record_at("fresh", now);

        store.save(&path)?;
        let loaded = FrecencyStore::load(&path);

        assert!(!loaded.entries.contains_key("stale"));
        assert!(loaded.entries.contains_key("fresh"));
        Ok(())
    }

    #[test]
    fn missing_file_loads_empty_store() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("absent.json");

        let mut loaded = FrecencyStore::load(&path);
        assert_eq!(loaded.entries.len(), 0);

        loaded.record("first");
        loaded.save(&path)?;

        assert_eq!(FrecencyStore::load(&path).entries.len(), 1);
        Ok(())
    }
}
