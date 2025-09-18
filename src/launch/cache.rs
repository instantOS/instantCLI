use anyhow::{Context, Result};
use fre::args::SortMethod;
use fre::store::{FrecencyStore, read_store, write_store};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::task;

use crate::launch::desktop::DesktopDiscovery;
use crate::launch::types::{LaunchItem, LaunchItemWithMetadata};

/// Application launcher cache for fast startup with background refresh
pub struct LaunchCache {
    cache_path: PathBuf,
    frecency_path: PathBuf,
    frecency_store: Option<FrecencyStore>,
    last_cache_check: Option<std::time::SystemTime>,
}

impl LaunchCache {
    /// Create a new launch cache instance
    pub fn new() -> Result<Self> {
        let cache_dir = if let Some(cache_dir) = dirs::cache_dir() {
            cache_dir.join("instant")
        } else {
            PathBuf::from(env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
                .join(".cache/instant")
        };

        // Ensure cache directory exists
        fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;

        let cache_path = cache_dir.join("launch_cache_v2");
        let frecency_path = cache_dir.join("frecency_store.json");

        Ok(Self {
            cache_path,
            frecency_path,
            frecency_store: None,
            last_cache_check: None,
        })
    }

    /// Get launch items with both desktop apps and PATH executables
    pub async fn get_launch_items(&mut self) -> Result<Vec<LaunchItemWithMetadata>> {
        // Check if we should skip cache freshness check (within 30 seconds)
        let skip_freshness_check = self.should_skip_cache_check();

        // Try to read cache first - fastest path
        if let Ok(items) = self.read_cache() {
            if skip_freshness_check || self.is_cache_fresh_fast()? {
                // Apply frecency sorting in-memory (much faster than filesystem access)
                return Ok(self.sort_metadata_by_frecency(items));
            }

            // Cache exists but is stale, use it for fast startup
            // Spawn background task to refresh cache
            let cache_path = self.cache_path.clone();
            task::spawn(async move {
                if let Err(e) = Self::refresh_cache_background(cache_path).await {
                    eprintln!("Warning: Failed to refresh application cache: {}", e);
                }
            });

            // Return stale cache immediately for fast startup
            return Ok(self.sort_metadata_by_frecency(items));
        }

        // No cache exists, need to do initial scan
        let items = self.scan_all_launch_items()?;

        // Write cache for next time (synchronous for now to ensure it works)
        if let Err(e) = Self::write_cache(&self.cache_path, &items) {
            eprintln!("Warning: Failed to write application cache: {}", e);
        }

        Ok(items)
    }

    /// Get display names for the menu (handles conflict resolution)
    pub async fn get_display_names(&mut self) -> Result<Vec<String>> {
        let items = self.get_launch_items().await?;
        let display_names = self.resolve_naming_conflicts(items);
        Ok(display_names)
    }

    /// Skip cache freshness check if we checked recently (within 30 seconds)
    fn should_skip_cache_check(&mut self) -> bool {
        if let Some(last_check) = self.last_cache_check {
            if let Ok(elapsed) = std::time::SystemTime::now().duration_since(last_check) {
                if elapsed.as_secs() < 30 {
                    return true;
                }
            }
        }
        false
    }

    /// Fast cache freshness check - only check cache file age, not all directories
    fn is_cache_fresh_fast(&mut self) -> Result<bool> {
        self.last_cache_check = Some(std::time::SystemTime::now());

        if !self.cache_path.exists() {
            return Ok(false);
        }

        // Just check if cache is less than 1 hour old
        if let Ok(metadata) = fs::metadata(&self.cache_path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = std::time::SystemTime::now().duration_since(modified) {
                    return Ok(elapsed.as_secs() < 3600); // 1 hour
                }
            }
        }

        Ok(false)
    }

    /// Check if cache is fresh by comparing with PATH and XDG directory modification times
    fn is_cache_fresh(&self) -> Result<bool> {
        if !self.cache_path.exists() {
            return Ok(false);
        }

        let cache_mtime = fs::metadata(&self.cache_path)?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);

        // Check if any PATH directory is newer than cache
        let path_env = env::var("PATH").unwrap_or_default();
        for path_dir in path_env.split(':') {
            if path_dir.is_empty() {
                continue;
            }

            let path = Path::new(path_dir);
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(dir_mtime) = metadata.modified() {
                    if dir_mtime > cache_mtime {
                        return Ok(false); // Directory is newer than cache
                    }
                }
            }
        }

        // Check if any XDG data directory is newer than cache
        let desktop_discovery = DesktopDiscovery::new()?;
        for data_dir in &desktop_discovery.data_dirs {
            let apps_dir = data_dir.join("applications");
            if apps_dir.exists() {
                if let Ok(metadata) = fs::metadata(&apps_dir) {
                    if let Ok(dir_mtime) = metadata.modified() {
                        if dir_mtime > cache_mtime {
                            return Ok(false); // Directory is newer than cache
                        }
                    }
                }
            }
        }

        Ok(true) // Cache is fresh
    }

    /// Read launch items from cache file
    fn read_cache(&self) -> Result<Vec<LaunchItemWithMetadata>> {
        if !self.cache_path.exists() {
            return Err(anyhow::anyhow!("Cache file does not exist"));
        }

        let content = fs::read_to_string(&self.cache_path).context("Failed to read cache file")?;

        let items: Vec<LaunchItemWithMetadata> =
            serde_json::from_str(&content).context("Failed to parse cache file")?;

        Ok(items)
    }

    /// Background task to refresh cache
    async fn refresh_cache_background(cache_path: PathBuf) -> Result<()> {
        let items = Self::scan_all_launch_items_static().await?;
        Self::write_cache(&cache_path, &items)?;
        Ok(())
    }

    /// Scan all launch items (desktop apps + PATH executables)
    fn scan_all_launch_items(&self) -> Result<Vec<LaunchItemWithMetadata>> {
        let mut items = Vec::new();

        // Scan desktop applications
        let desktop_discovery = DesktopDiscovery::new()?;
        let desktop_apps = desktop_discovery.discover_applications()?;

        for app in desktop_apps {
            let item = LaunchItem::DesktopApp(app);
            let metadata = LaunchItemWithMetadata::new(item);
            items.push(metadata);
        }

        // Scan PATH executables
        let path_executables = self.scan_path_executables()?;
        for executable in path_executables {
            let item = LaunchItem::PathExecutable(executable);
            let metadata = LaunchItemWithMetadata::new(item);
            items.push(metadata);
        }

        // Sort by name using pre-computed lowercase names
        items.sort_by(|a, b| {
            a.item.display_name_lower().cmp(&b.item.display_name_lower())
        });

        Ok(items)
    }

    /// Static version for async background task
    async fn scan_all_launch_items_static() -> Result<Vec<LaunchItemWithMetadata>> {
        let mut items = Vec::new();

        // Scan desktop applications
        let desktop_discovery = DesktopDiscovery::new()?;
        let desktop_apps = desktop_discovery.discover_applications()?;

        for app in desktop_apps {
            let item = LaunchItem::DesktopApp(app);
            let metadata = LaunchItemWithMetadata::new(item);
            items.push(metadata);
        }

        // Scan PATH executables
        let path_executables = Self::scan_path_executables_static().await?;
        for executable in path_executables {
            let item = LaunchItem::PathExecutable(executable);
            let metadata = LaunchItemWithMetadata::new(item);
            items.push(metadata);
        }

        // Sort by name using pre-computed lowercase names
        items.sort_by(|a, b| {
            a.item.display_name_lower().cmp(&b.item.display_name_lower())
        });

        Ok(items)
    }

    /// Scan PATH directories for executables
    fn scan_path_executables(&self) -> Result<Vec<String>> {
        let path_env = env::var("PATH").unwrap_or_default();
        let mut executables = HashSet::new();

        for path_dir in path_env.split(':') {
            if path_dir.is_empty() {
                continue;
            }

            let path = Path::new(path_dir);
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_file() {
                            // Check if file is executable
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                let permissions = metadata.permissions();
                                if permissions.mode() & 0o111 != 0 {
                                    if let Some(name) = entry.file_name().to_str() {
                                        executables.insert(name.to_string());
                                    }
                                }
                            }

                            #[cfg(not(unix))]
                            {
                                if let Some(name) = entry.file_name().to_str() {
                                    executables.insert(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort alphabetically and convert to Vec
        let mut apps: Vec<String> = executables.into_iter().collect();
        apps.sort();

        Ok(apps)
    }

    /// Static version of scan_path_executables for async context
    async fn scan_path_executables_static() -> Result<Vec<String>> {
        let path_env = env::var("PATH").unwrap_or_default();
        let mut executables = HashSet::new();

        for path_dir in path_env.split(':') {
            if path_dir.is_empty() {
                continue;
            }

            let path = Path::new(path_dir);
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_file() {
                            // Check if file is executable
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                let permissions = metadata.permissions();
                                if permissions.mode() & 0o111 != 0 {
                                    if let Some(name) = entry.file_name().to_str() {
                                        executables.insert(name.to_string());
                                    }
                                }
                            }

                            #[cfg(not(unix))]
                            {
                                if let Some(name) = entry.file_name().to_str() {
                                    executables.insert(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort alphabetically and convert to Vec
        let mut apps: Vec<String> = executables.into_iter().collect();
        apps.sort();

        Ok(apps)
    }

    /// Write launch items to cache file
    fn write_cache(cache_path: &Path, items: &[LaunchItemWithMetadata]) -> Result<()> {
        let content = serde_json::to_string(items).context("Failed to serialize cache items")?;
        fs::write(cache_path, content).context("Failed to write cache file")?;
        Ok(())
    }

    /// Resolve naming conflicts between desktop apps and PATH executables
    fn resolve_naming_conflicts(&mut self, items: Vec<LaunchItemWithMetadata>) -> Vec<String> {
        let mut name_to_items: HashMap<String, Vec<LaunchItem>> = HashMap::new();
        let mut name_to_metadata: HashMap<String, Vec<LaunchItemWithMetadata>> = HashMap::new();

        // Group items by name
        for item_with_metadata in items {
            let name = item_with_metadata.item.display_name();
            name_to_items
                .entry(name.to_string())
                .or_default()
                .push(item_with_metadata.item.clone());
            name_to_metadata
                .entry(name.to_string())
                .or_default()
                .push(item_with_metadata);
        }

        let mut display_names = Vec::new();

        // Apply frecency sorting to the items first
        let all_items: Vec<LaunchItemWithMetadata> = name_to_metadata
            .values()
            .flat_map(|items| items.iter())
            .cloned()
            .collect();

        let sorted_items = self.sort_metadata_by_frecency(all_items);

        // Create a frecency-based order for names
        let mut name_order: HashMap<String, usize> = HashMap::new();
        for (index, item) in sorted_items.iter().enumerate() {
            let name = item.item.display_name().to_string();
            name_order.insert(name, index);
        }

        // Process each group and maintain frecency order
        let mut name_groups: Vec<(String, Vec<LaunchItem>)> = name_to_items.into_iter().collect();

        // Sort the groups by the highest frecency position of any item in the group
        name_groups.sort_by(|(name_a, _), (name_b, _)| {
            let pos_a = name_order.get(name_a).unwrap_or(&usize::MAX);
            let pos_b = name_order.get(name_b).unwrap_or(&usize::MAX);
            pos_a.cmp(pos_b)
        });

        for (name, item_group) in name_groups {
            if item_group.len() == 1 {
                // No conflict, use the name as-is
                display_names.push(name);
            } else {
                // Conflict detected, handle it
                let mut has_desktop_app = false;
                let mut has_path_executable = false;

                for item in &item_group {
                    match item {
                        LaunchItem::DesktopApp(_) => has_desktop_app = true,
                        LaunchItem::PathExecutable(_) => has_path_executable = true,
                    }
                }

                if has_desktop_app && has_path_executable {
                    // Add both, prefix PATH executables with "path:"
                    for item in item_group {
                        match item {
                            LaunchItem::DesktopApp(_) => {
                                display_names.push(name.clone());
                            }
                            LaunchItem::PathExecutable(_) => {
                                display_names.push(format!("path:{}", name));
                            }
                        }
                    }
                } else {
                    // Multiple items of same type, just use name
                    display_names.push(name);
                }
            }
        }

        display_names
    }

    
    /// Sort LaunchItemWithMetadata by frecency
    fn sort_metadata_by_frecency(&mut self, mut items: Vec<LaunchItemWithMetadata>) -> Vec<LaunchItemWithMetadata> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        if let Ok(frecency_store) = self.get_frecency_store() {
            let sorted_items = frecency_store.sorted(fre::args::SortMethod::Frecent);

            // Create a frecency map for O(1) lookups instead of cloning items
            let mut frecency_map: HashMap<String, f64> = HashMap::new();
            for frecency_item in sorted_items {
                let item_name = frecency_item.item.clone();
                let frecency_score = frecency_item.get_frecency(current_time);
                frecency_map.insert(item_name, frecency_score);
            }

            // Pre-compute lowercase names to avoid repeated allocations during sorting
            let mut items_with_lower: Vec<(f64, String, &mut LaunchItemWithMetadata)> = items
                .iter_mut()
                .map(|item| {
                    let name = item.item.display_name();
                    let lower_name = name.to_lowercase();
                    let score = frecency_map.get(name).copied().unwrap_or(0.0);
                    item.frecency_score = score;
                    (score, lower_name, item)
                })
                .collect();

            // Sort by frecency (primary) then alphabetical (secondary)
            items_with_lower.sort_by(|(a_score, a_name, _), (b_score, b_name, _)| {
                if *a_score > 0.0 || *b_score > 0.0 {
                    b_score.partial_cmp(a_score).unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    a_name.cmp(b_name)
                }
            });

            // No need to extract back since we used references
        } else {
            // No frecency store, sort alphabetically with pre-computed lowercase names
            items.sort_by(|a, b| {
                a.item.display_name_lower().cmp(&b.item.display_name_lower())
            });
        }

        items
    }

    /// Get or initialize frecency store
    fn get_frecency_store(&mut self) -> Result<&mut FrecencyStore> {
        if self.frecency_store.is_none() {
            self.frecency_store = Some(read_store(&self.frecency_path).unwrap_or_default());
        }
        Ok(self.frecency_store.as_mut().unwrap())
    }

    /// Sort applications by frecency
    fn sort_by_frecency(&mut self, apps: &mut Vec<String>) -> Result<()> {
        let frecency_store = self.get_frecency_store()?;

        // Get sorted items from frecency store
        let sorted_items = frecency_store.sorted(SortMethod::Frecent);

        // Create a position map for O(1) lookups instead of O(N) searches
        let position_map: std::collections::HashMap<&String, usize> = sorted_items
            .iter()
            .enumerate()
            .map(|(idx, item)| (&item.item, idx))
            .collect();

        // Create a set of frequently used apps for fast lookup
        let frequent_apps: std::collections::HashSet<_> =
            sorted_items.iter().map(|item| &item.item).collect();

        // Sort apps: frequent apps first (in frecency order), then others alphabetically
        apps.sort_by(|a, b| {
            let a_is_frequent = frequent_apps.contains(a);
            let b_is_frequent = frequent_apps.contains(b);

            match (a_is_frequent, b_is_frequent) {
                (true, true) => {
                    // Both are frequent, sort by frecency order using position map
                    let a_index = position_map.get(a).copied().unwrap_or(usize::MAX);
                    let b_index = position_map.get(b).copied().unwrap_or(usize::MAX);
                    a_index.cmp(&b_index)
                }
                (true, false) => std::cmp::Ordering::Less, // Frequent apps come first
                (false, true) => std::cmp::Ordering::Greater, // Infrequent apps come later
                (false, false) => a.cmp(b),                // Both infrequent, sort alphabetically
            }
        });

        Ok(())
    }

    /// Record application launch in frecency store (optimized)
    pub fn record_launch(&mut self, app_name: &str) -> Result<()> {
        // Use a more efficient approach - update frecency in memory, save asynchronously
        let frecency_store = self.get_frecency_store()?;
        frecency_store.add(app_name);

        // Save in background to avoid blocking UI (only if we've had a few launches)
        if frecency_store.items.len() % 5 == 0 {
            let frecency_path = self.frecency_path.clone();
            // Create a new store instance for background saving
            if let Ok(new_store) = read_store(&self.frecency_path) {
                task::spawn(async move {
                    if let Err(e) = write_store(new_store, &frecency_path) {
                        eprintln!("Warning: Failed to save frecency store: {}", e);
                    }
                });
            }
        }

        Ok(())
    }

    /// Save frecency store to disk
    fn save_frecency_store(&mut self) -> Result<()> {
        if let Some(store) = self.frecency_store.take() {
            write_store(store, &self.frecency_path).context("Failed to save frecency store")?;
            // Reload the store after saving
            self.frecency_store = Some(read_store(&self.frecency_path).unwrap_or_default());
        }
        Ok(())
    }

    /// Get frecency statistics for debugging
    pub fn get_frecency_stats(&mut self) -> Result<String> {
        let frecency_store = self.get_frecency_store()?;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        let mut stats = String::new();
        stats.push_str(&format!(
            "Frecency Store ({} items):\n",
            frecency_store.items.len()
        ));

        for item in frecency_store.items.iter().take(10) {
            let frecency = item.get_frecency(current_time);
            stats.push_str(&format!(
                "  {}: {:.2} (accessed {} times)\n",
                item.item, frecency, item.num_accesses
            ));
        }

        if frecency_store.items.len() > 10 {
            stats.push_str(&format!(
                "  ... and {} more\n",
                frecency_store.items.len() - 10
            ));
        }

        Ok(stats)
    }
}
