use crate::game::config::InstantGameConfig;
use crate::restic::wrapper::Snapshot;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::OnceLock;
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

/// Global snapshot cache using OnceLock for thread safety
static SNAPSHOT_CACHE: OnceLock<HashMap<String, CachedSnapshots>> = OnceLock::new();

/// Get or initialize the snapshot cache
fn get_cache() -> &'static HashMap<String, CachedSnapshots> {
    SNAPSHOT_CACHE.get_or_init(HashMap::new)
}

/// Generate cache key for a game and repository
fn generate_cache_key(repository_path: &str, game_name: &str) -> String {
    format!("{repository_path}:{game_name}")
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
    let repository_path = config.repo.as_path().to_string_lossy().to_string();
    let cache_key = generate_cache_key(&repository_path, game_name);

    // Check cache first
    let cache = get_cache();
    if let Some(cached_data) = cache.get(&cache_key) {
        if is_cache_valid(cached_data) && cached_data.repository_path == repository_path {
            return Ok(cached_data.snapshots.clone());
        }
    }

    // Cache miss or invalid, fetch fresh data
    let snapshots = fetch_snapshots_from_restic(game_name, config)?;

    // Update cache
    let mut new_cache = HashMap::new();
    if let Some(old_cache) = SNAPSHOT_CACHE.get() {
        new_cache.clone_from(old_cache);
    }

    new_cache.insert(
        cache_key,
        CachedSnapshots {
            snapshots: snapshots.clone(),
            timestamp: SystemTime::now(),
            repository_path,
        },
    );

    // This is safe because we're in a single-threaded context or the first call
    let _ = SNAPSHOT_CACHE.set(new_cache);

    Ok(snapshots)
}

/// Fetch fresh snapshots from restic (expensive operation)
fn fetch_snapshots_from_restic(
    game_name: &str,
    config: &InstantGameConfig,
) -> Result<Vec<Snapshot>> {
    let restic = crate::restic::wrapper::ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    );

    let snapshots_json = restic
        .list_snapshots_filtered(Some(vec!["instantgame".to_string(), game_name.to_string()]))
        .context("Failed to list snapshots for game")?;

    let mut parsed_snapshots: Vec<Snapshot> =
        serde_json::from_str(&snapshots_json).context("Failed to parse snapshot data")?;

    // Sort by date (newest first)
    parsed_snapshots.sort_by(|a, b| b.time.cmp(&a.time));

    Ok(parsed_snapshots)
}

/// Invalidate the entire snapshot cache
pub fn invalidate_snapshot_cache() {
    let _ = SNAPSHOT_CACHE.set(HashMap::new());
}

/// Invalidate cache for a specific game
pub fn invalidate_game_cache(game_name: &str, repository_path: &str) {
    if let Some(mut cache) = SNAPSHOT_CACHE.get().cloned() {
        let cache_key = generate_cache_key(repository_path, game_name);
        cache.remove(&cache_key);
        let _ = SNAPSHOT_CACHE.set(cache);
    }
}

/// Get cache statistics for debugging
pub fn get_cache_stats() -> String {
    let cache = get_cache();
    let _now = SystemTime::now();

    let mut valid_entries = 0;
    let mut expired_entries = 0;

    for cached_data in cache.values() {
        if is_cache_valid(cached_data) {
            valid_entries += 1;
        } else {
            expired_entries += 1;
        }
    }

    format!(
        "Snapshot Cache Stats: {} total entries, {} valid, {} expired",
        cache.len(),
        valid_entries,
        expired_entries
    )
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
