//! Launch history for the game menu.
//!
//! Every launched game bumps its score in a small frecency cache. The game
//! menu reads it to list recently and frequently played games first. The
//! cache is advisory: it must never block or fail a launch.

use std::fs::OpenOptions;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nix::fcntl::{Flock, FlockArg};

use crate::frecency::FrecencyStore;

const CACHE_FILE: &str = "game-frecency.json";

fn cache_path() -> Result<PathBuf> {
    let dir = dirs::cache_dir().context("Unable to resolve cache directory")?;
    Ok(dir.join("instant").join(CACHE_FILE))
}

/// Frecency scores of launched games, keyed by game name.
pub struct LaunchHistory {
    store: FrecencyStore,
}

impl LaunchHistory {
    /// Load the history for ranking. A missing or unreadable cache yields an
    /// empty history, so the menu always opens.
    pub fn load() -> Self {
        let store = cache_path()
            .map(|path| FrecencyStore::load(&path))
            .unwrap_or_default();
        Self { store }
    }

    /// How recently and often `game` was launched; 0.0 for a game that was
    /// never launched.
    pub fn score(&self, game: &str) -> f64 {
        self.store.score(game)
    }
}

/// Remember that `game` was launched now. Cache problems are reported but
/// never prevent the launch.
pub fn record_launch(game: &str) {
    if let Err(error) = update(game) {
        eprintln!("Warning: could not update game launch history: {error:#}");
    }
}

fn update(game: &str) -> Result<()> {
    let path = cache_path()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("Launch history path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create {}", parent.display()))?;

    // Serialize read/modify/write across launchers that may run at once.
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))
        .context("Cannot open game launch history lock")?;
    let _lock = Flock::lock(lock, FlockArg::LockExclusive)
        .map_err(|(_, error)| error)
        .context("Cannot lock game launch history")?;

    let mut store = FrecencyStore::load(&path);
    store.record(game);
    store.save(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot::test_util::EnvGuard;

    #[test]
    #[serial_test::serial]
    fn launches_are_cached_and_rank_their_games() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let _cache = EnvGuard::set("XDG_CACHE_HOME", temp.path());
        let path = cache_path()?;
        assert!(!path.exists());

        record_launch("Zeta");
        record_launch("Alpha");
        record_launch("Zeta");

        assert!(path.is_file());
        let history = LaunchHistory::load();
        assert!(history.score("Zeta") > history.score("Alpha"));
        assert!(history.score("Alpha") > 0.0);
        assert_eq!(history.score("Never launched"), 0.0);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn unreadable_cache_never_breaks_ranking_or_launches() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let _cache = EnvGuard::set("XDG_CACHE_HOME", temp.path());
        let path = cache_path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, "not json")?;

        assert_eq!(LaunchHistory::load().score("Alpha"), 0.0);

        // A discarded cache is replaced by the next launch.
        record_launch("Alpha");
        assert!(LaunchHistory::load().score("Alpha") > 0.0);
        Ok(())
    }
}
