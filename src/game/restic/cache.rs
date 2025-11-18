use crate::game::config::InstantGameConfig;
use crate::game::restic::tags;
use crate::restic::wrapper::Snapshot;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Cache duration in seconds (5 minutes)
const CACHE_DURATION: u64 = 300;

/// Cached snapshot data with timestamp
#[derive(Debug, Clone)]
pub struct CachedSnapshots {
    pub snapshots: Vec<Snapshot>,
    pub timestamp: SystemTime,
    pub repository_path: String,
}

#[derive(Debug, Default)]
struct SnapshotCacheState {
    repositories: HashMap<String, CachedSnapshots>,
}

/// Global snapshot cache using OnceLock for thread safety
static SNAPSHOT_CACHE: OnceLock<Mutex<SnapshotCacheState>> = OnceLock::new();

/// Get or initialize the snapshot cache
fn get_cache() -> &'static Mutex<SnapshotCacheState> {
    SNAPSHOT_CACHE.get_or_init(|| Mutex::new(SnapshotCacheState::default()))
}

/// Check if cached data is still valid (not expired)
fn is_cache_valid(cached_data: &CachedSnapshots) -> bool {
    let now = SystemTime::now();
    match now.duration_since(cached_data.timestamp) {
        Ok(duration) => duration.as_secs() < CACHE_DURATION,
        Err(_) => false, // Clock went backwards, cache is invalid
    }
}

/// Get snapshots for a game, using cache if available and valid
pub fn get_snapshots_for_game(
    game_name: &str,
    config: &InstantGameConfig,
) -> Result<Vec<Snapshot>> {
    let required_tags = tags::create_game_tags(game_name);
    let mut filtered_snapshots: Vec<Snapshot> = get_repository_snapshots(config)?
        .into_iter()
        .filter(|snapshot| {
            required_tags
                .iter()
                .all(|tag| snapshot.tags.iter().any(|existing| existing == tag))
        })
        .collect();

    filtered_snapshots.sort_by(|a, b| b.time.cmp(&a.time));

    Ok(filtered_snapshots)
}

/// Retrieve all snapshots for the configured repository, using the in-memory cache when possible
pub fn get_repository_snapshots(config: &InstantGameConfig) -> Result<Vec<Snapshot>> {
    let repository_path = config.repo.as_path().to_string_lossy().to_string();

    if let Some(cached) = get_cached_repository_snapshots(&repository_path)? {
        return Ok(cached);
    }

    let fetched = fetch_repository_snapshots_from_restic(config)?;
    store_repository_snapshots(&repository_path, fetched.clone())?;
    Ok(fetched)
}

fn get_cached_repository_snapshots(repository_path: &str) -> Result<Option<Vec<Snapshot>>> {
    let cache = get_cache()
        .lock()
        .map_err(|_| anyhow!("Snapshot cache mutex poisoned"))?;

    Ok(cache
        .repositories
        .get(repository_path)
        .filter(|cached| is_cache_valid(cached) && cached.repository_path == repository_path)
        .map(|cached| cached.snapshots.clone()))
}

fn store_repository_snapshots(repository_path: &str, snapshots: Vec<Snapshot>) -> Result<()> {
    let mut cache = get_cache()
        .lock()
        .map_err(|_| anyhow!("Snapshot cache mutex poisoned"))?;

    cache.repositories.insert(
        repository_path.to_string(),
        CachedSnapshots {
            snapshots,
            timestamp: SystemTime::now(),
            repository_path: repository_path.to_string(),
        },
    );

    Ok(())
}

/// Fetch fresh snapshots for entire repository from restic (expensive operation)
fn fetch_repository_snapshots_from_restic(config: &InstantGameConfig) -> Result<Vec<Snapshot>> {
    let restic = crate::restic::wrapper::ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    )
    .context("Failed to initialize restic wrapper")?;

    restic
        .list_snapshots()
        .context("Failed to list snapshots for repository")
}

/// Invalidate the entire snapshot cache
pub fn invalidate_snapshot_cache() {
    if let Ok(mut cache) = get_cache().lock() {
        cache.repositories.clear();
    }
}

/// Invalidate cache for a specific game
pub fn invalidate_game_cache(game_name: &str, repository_path: &str) {
    let _ = game_name; // game-level cache is derived from repository snapshots

    if let Ok(mut cache) = get_cache().lock() {
        cache.repositories.remove(repository_path);
    }
}

/// Get cache statistics for debugging
pub fn get_cache_stats() -> String {
    if let Ok(cache) = get_cache().lock() {
        let _now = SystemTime::now();

        let mut valid_entries = 0;
        let mut expired_entries = 0;

        for cached_data in cache.repositories.values() {
            if is_cache_valid(cached_data) {
                valid_entries += 1;
            } else {
                expired_entries += 1;
            }
        }

        format!(
            "Snapshot Cache Stats: {} total entries, {} valid, {} expired",
            cache.repositories.len(),
            valid_entries,
            expired_entries
        )
    } else {
        "Snapshot Cache Stats: cache unavailable".to_string()
    }
}

/// Force refresh snapshots for a specific game
pub fn refresh_snapshots_for_game(
    game_name: &str,
    config: &InstantGameConfig,
) -> Result<Vec<Snapshot>> {
    // Invalidate cache for this game first
    let repository_path = config.repo.as_path().to_string_lossy().to_string();
    invalidate_game_cache(game_name, &repository_path);

    // Fetch fresh data
    get_snapshots_for_game(game_name, config)
}

/// Get snapshot by ID from cached snapshots if available
pub fn get_snapshot_by_id(
    snapshot_id: &str,
    game_name: &str,
    config: &InstantGameConfig,
) -> Result<Option<Snapshot>> {
    let snapshots = get_snapshots_for_game(game_name, config)?;

    Ok(snapshots
        .into_iter()
        .find(|snapshot| snapshot.id == snapshot_id))
}
