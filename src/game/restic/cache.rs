use crate::game::config::InstantGameConfig;
use crate::game::ownership::OwnerId;
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
    get_snapshots_for_owner(&config.owner_id(game_name), config)
}

pub fn get_snapshots_for_owner(
    owner: &OwnerId,
    config: &InstantGameConfig,
) -> Result<Vec<Snapshot>> {
    filter_owner(&get_repository_snapshots(config)?, owner)
}

fn filter_owner(snapshots: &[Snapshot], owner: &OwnerId) -> Result<Vec<Snapshot>> {
    let required_tags = owner.tags();
    let mut filtered_snapshots: Vec<Snapshot> = snapshots
        .iter()
        .filter(|snapshot| {
            required_tags
                .iter()
                .all(|tag| snapshot.tags.iter().any(|existing| existing == tag))
        })
        .cloned()
        .collect();

    let mut keyed = filtered_snapshots
        .drain(..)
        .map(|snapshot| {
            let time = chrono::DateTime::parse_from_rfc3339(&snapshot.time)
                .context("Snapshot has an invalid creation time")?;
            Ok((time, snapshot))
        })
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|(a_time, a), (b_time, b)| b_time.cmp(a_time).then_with(|| b.id.cmp(&a.id)));
    Ok(keyed.into_iter().map(|(_, snapshot)| snapshot).collect())
}

/// Retrieve all snapshots for the configured repository, using the in-memory cache when possible
pub fn get_repository_snapshots(config: &InstantGameConfig) -> Result<Vec<Snapshot>> {
    let repository_path = config.repo.as_path().to_string_lossy().to_string();

    if let Some(cached) = get_cached_snapshots(&repository_path)? {
        return Ok(cached);
    }

    let fetched = fetch_snapshots(config)?;
    store_snapshots(&repository_path, fetched.clone())?;
    Ok(fetched)
}

fn get_cached_snapshots(repository_path: &str) -> Result<Option<Vec<Snapshot>>> {
    let cache = get_cache()
        .lock()
        .map_err(|_| anyhow!("Snapshot cache mutex poisoned"))?;

    Ok(cache
        .repositories
        .get(repository_path)
        .filter(|cached| is_cache_valid(cached) && cached.repository_path == repository_path)
        .map(|cached| cached.snapshots.clone()))
}

fn store_snapshots(repository_path: &str, snapshots: Vec<Snapshot>) -> Result<()> {
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
fn fetch_snapshots(config: &InstantGameConfig) -> Result<Vec<Snapshot>> {
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

pub fn get_snapshot_for_owner(
    snapshot_id: &str,
    owner: &OwnerId,
    config: &InstantGameConfig,
) -> Result<Option<Snapshot>> {
    Ok(get_snapshots_for_owner(owner, config)?
        .into_iter()
        .find(|snapshot| snapshot.matches_id(snapshot_id)))
}

/// One explicit repository observation per sync pass, shared by every owner.
/// Unlike the display cache, this view never starts from a previous operation's
/// TTL cache. Uploads invalidate it before running and refresh it afterwards.
#[derive(Default)]
pub(crate) struct SnapshotView {
    snapshots: Option<Vec<Snapshot>>,
}

impl SnapshotView {
    pub(crate) fn for_owner(
        &mut self,
        config: &InstantGameConfig,
        owner: &OwnerId,
    ) -> Result<Vec<Snapshot>> {
        if self.snapshots.is_none() {
            self.refresh(config)?;
        }
        filter_owner(
            self.snapshots
                .as_deref()
                .context("Snapshot view was not loaded")?,
            owner,
        )
    }

    pub(crate) fn invalidate(&mut self) {
        self.snapshots = None;
    }

    pub(crate) fn refresh(&mut self, config: &InstantGameConfig) -> Result<()> {
        self.invalidate();
        let snapshots = fetch_snapshots(config)?;
        store_snapshots(&config.repo.as_path().to_string_lossy(), snapshots.clone())?;
        self.snapshots = Some(snapshots);
        Ok(())
    }
}
