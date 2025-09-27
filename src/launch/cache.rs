use anyhow::{Context, Result};
use fre::args::SortMethod;
use fre::store::{FrecencyStore, read_store, write_store};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::task;

use crate::launch::types::LaunchItem;

/// Application launcher cache for fast startup with background refresh
pub struct LaunchCache {
    cache_path: PathBuf,
    frecency_path: PathBuf,
    launch_items_path: PathBuf,
    frecency_sorted_path: PathBuf,
    frecency_store: Option<FrecencyStore>,
}

impl LaunchCache {
    /// Create a new launch cache instance
    pub fn new() -> Result<Self> {
        let cache_dir = if let Some(cache_dir) = dirs::cache_dir() {
            cache_dir.join(env!("CARGO_BIN_NAME"))
        } else {
            PathBuf::from(env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
                .join(format!(".cache/{}", env!("CARGO_BIN_NAME")))
        };

        // Ensure cache directory exists
        fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;

        let cache_path = cache_dir.join("launch_cache");
        let frecency_path = cache_dir.join("frecency_store.json");
        let launch_items_path = cache_dir.join("launch_items_cache");
        let frecency_sorted_path = cache_dir.join("frecency_sorted_cache");

        Ok(Self {
            cache_path,
            frecency_path,
            launch_items_path,
            frecency_sorted_path,
            frecency_store: None,
        })
    }

    /// Get launch items - extremely fast path for menu display
    pub async fn get_launch_items(&mut self) -> Result<Vec<LaunchItem>> {
        // Always use frecency-sorted cache if available (even if stale)
        if let Ok(sorted_items) = self.read_frecency_sorted_cache() {
            if !sorted_items.is_empty() {
                // Background refresh if any underlying data is stale
                if !self.is_launch_cache_fresh()? || !self.is_frecency_sorted_cache_fresh()? {
                    self.trigger_background_refresh_and_resort();
                }
                return Ok(sorted_items);
            }
        }

        // No frecency-sorted cache exists, build from regular cache
        let cached_items = self.read_launch_cache().unwrap_or_default();

        // Background refresh if underlying data is stale
        if !self.is_launch_cache_fresh()? {
            self.trigger_background_refresh();
        }

        // Apply frecency sorting and return
        let mut items = cached_items;

        // If cache is empty, build items synchronously for first run
        if items.is_empty() {
            items = Self::build_item_list_simple();
        }

        self.sort_by_frecency_launch_items(&mut items)?;

        // Cache the sorted result for future use
        self.save_frecency_sorted_cache(&items)?;

        Ok(items)
    }

    /// Get applications with dmenu-style caching strategy (legacy PATH-only)
    pub async fn get_applications(&mut self) -> Result<Vec<String>> {
        // Check if cache is fresh
        if self.is_cache_fresh()? {
            // Use fresh cache with frecency sorting
            let mut apps = self.read_cache()?;
            self.sort_by_frecency(&mut apps)?;
            Ok(apps)
        } else {
            // Cache is stale or doesn't exist
            let stale_apps = self.read_cache().unwrap_or_default();

            // Spawn background task to refresh cache
            let cache_path = self.cache_path.clone();
            task::spawn(async move {
                if let Err(e) = Self::refresh_cache_background(cache_path).await {
                    eprintln!("Warning: Failed to refresh application cache: {e}");
                }
            });

            // Return stale cache immediately for fast startup
            let mut apps = if stale_apps.is_empty() {
                // If no stale cache, do a quick scan now
                self.scan_path_directories()?
            } else {
                stale_apps
            };

            // Sort by frecency
            self.sort_by_frecency(&mut apps)?;
            Ok(apps)
        }
    }

    /// Check if cache is fresh by comparing with PATH directory modification times
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

        Ok(true) // Cache is fresh
    }

    /// Read applications from cache file
    fn read_cache(&self) -> Result<Vec<String>> {
        if !self.cache_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.cache_path).context("Failed to read cache file")?;

        let apps: Vec<String> = content
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();

        Ok(apps)
    }

    /// Background task to refresh cache
    async fn refresh_cache_background(cache_path: PathBuf) -> Result<()> {
        let apps = Self::scan_path_directories_static().await?;
        Self::write_cache(&cache_path, &apps)?;
        Ok(())
    }

    /// Scan PATH directories for executables
    fn scan_path_directories(&self) -> Result<Vec<String>> {
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

    /// Static version of scan_path_directories for async context
    async fn scan_path_directories_static() -> Result<Vec<String>> {
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

    /// Write applications to cache file
    fn write_cache(cache_path: &Path, apps: &[String]) -> Result<()> {
        let content = apps.join("\n");
        fs::write(cache_path, content).context("Failed to write cache file")?;
        Ok(())
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

        // Create a set of frequently used apps for fast lookup
        let frequent_apps: std::collections::HashSet<_> =
            sorted_items.iter().map(|item| &item.item).collect();

        // Sort apps: frequent apps first (in frecency order), then others alphabetically
        apps.sort_by(|a, b| {
            let a_is_frequent = frequent_apps.contains(a);
            let b_is_frequent = frequent_apps.contains(b);

            match (a_is_frequent, b_is_frequent) {
                (true, true) => {
                    // Both are frequent, sort by frecency order
                    let a_index = sorted_items
                        .iter()
                        .position(|item| &item.item == a)
                        .unwrap_or(0);
                    let b_index = sorted_items
                        .iter()
                        .position(|item| &item.item == b)
                        .unwrap_or(0);
                    a_index.cmp(&b_index)
                }
                (true, false) => std::cmp::Ordering::Less, // Frequent apps come first
                (false, true) => std::cmp::Ordering::Greater, // Infrequent apps come later
                (false, false) => a.cmp(b),                // Both infrequent, sort alphabetically
            }
        });

        Ok(())
    }

    /// Record application launch in frecency store
    pub fn record_launch(&mut self, app_name: &str) -> Result<()> {
        let frecency_store = self.get_frecency_store()?;
        frecency_store.add(app_name);
        self.save_frecency_store()?;
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

    // === Desktop Support Methods ===

    /// Read launch items from cache file
    fn read_launch_cache(&self) -> Result<Vec<LaunchItem>> {
        if !self.launch_items_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.launch_items_path)
            .context("Failed to read launch items cache file")?;

        let items: Result<Vec<LaunchItem>> = content
            .lines()
            .map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return Err(anyhow::anyhow!("Empty line"));
                }

                if let Some(desktop_id) = line.strip_prefix("desktop:") {
                    Ok(LaunchItem::DesktopApp(desktop_id.to_string()))
                } else if let Some(exec_name) = line.strip_prefix("path:") {
                    Ok(LaunchItem::PathExecutable(exec_name.to_string()))
                } else {
                    // Default to path executable for backward compatibility
                    Ok(LaunchItem::PathExecutable(line.to_string()))
                }
            })
            .collect();

        items
    }

    /// Check if launch items cache is fresh
    fn is_launch_cache_fresh(&self) -> Result<bool> {
        if !self.launch_items_path.exists() {
            return Ok(false);
        }

        let cache_mtime = fs::metadata(&self.launch_items_path)?
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

        // Check XDG data directories for desktop files
        let data_dirs = Self::get_xdg_data_dirs();
        for data_dir in data_dirs {
            let apps_dir = data_dir.join("applications");
            if apps_dir.exists() {
                if let Ok(metadata) = fs::metadata(&apps_dir) {
                    if let Ok(dir_mtime) = metadata.modified() {
                        if dir_mtime > cache_mtime {
                            return Ok(false); // Apps directory is newer than cache
                        }
                    }
                }
            }
        }

        Ok(true) // Cache is fresh
    }

    /// Trigger background refresh of launch items cache
    fn trigger_background_refresh(&self) {
        let cache_path = self.launch_items_path.clone();
        task::spawn(async move {
            let items = Self::build_item_list_simple();
            if let Err(e) = Self::save_launch_items_cache_simple(cache_path, items) {
                eprintln!("Warning: Failed to refresh launch items cache: {e}");
            }
        });
    }

    /// Trigger background refresh and resort of both caches
    fn trigger_background_refresh_and_resort(&self) {
        let launch_cache_path = self.launch_items_path.clone();
        let frecency_cache_path = self.frecency_sorted_path.clone();
        let frecency_store_path = self.frecency_path.clone();

        task::spawn(async move {
            // Refresh the base launch items cache
            let items = Self::build_item_list_simple();
            if let Err(e) =
                Self::save_launch_items_cache_simple(launch_cache_path.clone(), items.clone())
            {
                eprintln!("Warning: Failed to refresh launch items cache: {e}");
                return;
            }

            // Load frecency store and resort the items
            let frecency_store = read_store(&frecency_store_path).unwrap_or_default();
            let sorted_items = frecency_store.sorted(fre::args::SortMethod::Frecent);

            let frequent_keys: std::collections::HashSet<_> =
                sorted_items.iter().map(|item| &item.item).collect();

            // Sort items by frecency
            let mut sorted_launch_items = items;
            sorted_launch_items.sort_by(|a, b| {
                let a_key = Self::get_frecency_key_static(a);
                let b_key = Self::get_frecency_key_static(b);

                let a_is_frequent = frequent_keys.contains(&a_key);
                let b_is_frequent = frequent_keys.contains(&b_key);

                match (a_is_frequent, b_is_frequent) {
                    (true, true) => {
                        let a_index = sorted_items
                            .iter()
                            .position(|item| item.item == a_key)
                            .unwrap_or(0);
                        let b_index = sorted_items
                            .iter()
                            .position(|item| item.item == b_key)
                            .unwrap_or(0);
                        a_index.cmp(&b_index)
                    }
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (false, false) => a.sort_key().cmp(&b.sort_key()),
                }
            });

            // Save the frecency-sorted cache
            if let Err(e) =
                Self::save_frecency_sorted_cache_static(frecency_cache_path, &sorted_launch_items)
            {
                eprintln!("Warning: Failed to refresh frecency sorted cache: {e}");
            }
        });
    }

    /// Build item list with minimal overhead
    fn build_item_list_simple() -> Vec<LaunchItem> {
        let mut items = Vec::new();

        // Get desktop app names (fast)
        let desktop_items = Self::get_desktop_names_fast();
        items.extend(desktop_items);

        // Get PATH executables (fast)
        let path_items = Self::get_path_names_fast();
        items.extend(path_items);

        // Simple conflict resolution
        Self::resolve_conflicts_simple(items)
    }

    /// Get XDG data directories
    fn get_xdg_data_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        if let Some(home_data) = dirs::data_dir() {
            dirs.push(home_data);
        }

        if let Ok(system_dirs) = env::var("XDG_DATA_DIRS") {
            for dir in system_dirs.split(':') {
                if !dir.is_empty() {
                    dirs.push(PathBuf::from(dir));
                }
            }
        } else {
            dirs.push(PathBuf::from("/usr/local/share"));
            dirs.push(PathBuf::from("/usr/share"));
        }

        dirs
    }

    /// Fast desktop name scanning - no parsing, just file names
    fn get_desktop_names_fast() -> Vec<LaunchItem> {
        let mut names = Vec::new();
        let data_dirs = Self::get_xdg_data_dirs();

        for data_dir in data_dirs {
            let apps_dir = data_dir.join("applications");
            if apps_dir.exists() {
                Self::scan_desktop_names_simple(&apps_dir, &mut names);
            }
        }

        names
    }

    /// Simple name scanning - skip parsing entirely
    fn scan_desktop_names_simple(apps_dir: &Path, names: &mut Vec<LaunchItem>) {
        if let Ok(entries) = fs::read_dir(apps_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        // Skip obvious test/debug files by filename
                        if !file_name.contains("test") && !file_name.contains("debug") {
                            names.push(LaunchItem::DesktopApp(file_name.to_string()));
                        }
                    }
                }
            }
        }
    }

    /// Get PATH executable names
    fn get_path_names_fast() -> Vec<LaunchItem> {
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

        // Convert to LaunchItems
        executables
            .into_iter()
            .map(LaunchItem::PathExecutable)
            .collect()
    }

    /// Simple conflict resolution
    fn resolve_conflicts_simple(items: Vec<LaunchItem>) -> Vec<LaunchItem> {
        let mut result = Vec::new();
        let desktop_names: std::collections::HashSet<_> = items
            .iter()
            .filter_map(|item| {
                if let LaunchItem::DesktopApp(id) = item {
                    Some(id.strip_suffix(".desktop").unwrap_or(id).to_lowercase())
                } else {
                    None
                }
            })
            .collect();

        for item in items {
            match item {
                LaunchItem::DesktopApp(_) => {
                    result.push(item);
                }
                LaunchItem::PathExecutable(name) => {
                    if desktop_names.contains(&name.to_lowercase()) {
                        // Add prefix to avoid conflict
                        result.push(LaunchItem::PathExecutable(format!("path:{name}")));
                    } else {
                        result.push(LaunchItem::PathExecutable(name));
                    }
                }
            }
        }

        result
    }

    /// Save launch items to cache file
    fn save_launch_items_cache_simple(cache_path: PathBuf, items: Vec<LaunchItem>) -> Result<()> {
        let content: Vec<String> = items
            .into_iter()
            .map(|item| match item {
                LaunchItem::DesktopApp(id) => format!("desktop:{id}"),
                LaunchItem::PathExecutable(name) => format!("path:{name}"),
            })
            .collect();

        fs::write(cache_path, content.join("\n"))?;
        Ok(())
    }

    /// Sort launch items by frecency
    fn sort_by_frecency_launch_items(&mut self, items: &mut Vec<LaunchItem>) -> Result<()> {
        let frecency_store = self.get_frecency_store()?;
        let sorted_items = frecency_store.sorted(SortMethod::Frecent);

        let frequent_keys: std::collections::HashSet<_> =
            sorted_items.iter().map(|item| &item.item).collect();

        items.sort_by(|a, b| {
            let a_key = self.get_frecency_key(a);
            let b_key = self.get_frecency_key(b);

            let a_is_frequent = frequent_keys.contains(&a_key);
            let b_is_frequent = frequent_keys.contains(&b_key);

            match (a_is_frequent, b_is_frequent) {
                (true, true) => {
                    let a_index = sorted_items
                        .iter()
                        .position(|item| item.item == a_key)
                        .unwrap_or(0);
                    let b_index = sorted_items
                        .iter()
                        .position(|item| item.item == b_key)
                        .unwrap_or(0);
                    a_index.cmp(&b_index)
                }
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (false, false) => a.sort_key().cmp(&b.sort_key()),
            }
        });

        Ok(())
    }

    /// Get frecency key for a launch item
    fn get_frecency_key(&self, item: &LaunchItem) -> String {
        match item {
            LaunchItem::DesktopApp(desktop_id) => desktop_id.clone(),
            LaunchItem::PathExecutable(name) => {
                // Remove "path:" prefix if present for frecency tracking
                name.strip_prefix("path:").unwrap_or(name).to_string()
            }
        }
    }

    /// Record launch item usage in frecency store
    pub fn record_launch_item(&mut self, item: &LaunchItem) -> Result<()> {
        let key = self.get_frecency_key(item);
        let frecency_store = self.get_frecency_store()?;
        frecency_store.add(&key);
        self.save_frecency_store()?;

        // Invalidate frecency-sorted cache when new launch is recorded
        self.invalidate_frecency_sorted_cache()?;

        Ok(())
    }

    /// Read frecency-sorted cache
    fn read_frecency_sorted_cache(&self) -> Result<Vec<LaunchItem>> {
        if !self.frecency_sorted_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.frecency_sorted_path)
            .context("Failed to read frecency sorted cache file")?;

        let items: Result<Vec<LaunchItem>> = content
            .lines()
            .map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return Err(anyhow::anyhow!("Empty line"));
                }

                if let Some(desktop_id) = line.strip_prefix("desktop:") {
                    Ok(LaunchItem::DesktopApp(desktop_id.to_string()))
                } else if let Some(exec_name) = line.strip_prefix("path:") {
                    Ok(LaunchItem::PathExecutable(exec_name.to_string()))
                } else {
                    // Default to path executable for backward compatibility
                    Ok(LaunchItem::PathExecutable(line.to_string()))
                }
            })
            .collect();

        items
    }

    /// Check if frecency-sorted cache is fresh (valid for 30 seconds)
    fn is_frecency_sorted_cache_fresh(&self) -> Result<bool> {
        if !self.frecency_sorted_path.exists() {
            return Ok(false);
        }

        let cache_mtime = fs::metadata(&self.frecency_sorted_path)?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let cache_age = SystemTime::now()
            .duration_since(cache_mtime)
            .unwrap_or(std::time::Duration::from_secs(0));

        // Cache is fresh for 30 seconds
        Ok(cache_age.as_secs() <= 30)
    }

    /// Save frecency-sorted cache
    fn save_frecency_sorted_cache(&self, items: &[LaunchItem]) -> Result<()> {
        let content: Vec<String> = items
            .iter()
            .map(|item| match item {
                LaunchItem::DesktopApp(id) => format!("desktop:{id}"),
                LaunchItem::PathExecutable(name) => format!("path:{name}"),
            })
            .collect();

        fs::write(&self.frecency_sorted_path, content.join("\n"))
            .context("Failed to write frecency sorted cache file")?;
        Ok(())
    }

    /// Invalidate frecency-sorted cache
    fn invalidate_frecency_sorted_cache(&self) -> Result<()> {
        if self.frecency_sorted_path.exists() {
            fs::remove_file(&self.frecency_sorted_path)
                .context("Failed to remove frecency sorted cache file")?;
        }
        Ok(())
    }

    /// Static version of get_frecency_key for background processing
    fn get_frecency_key_static(item: &LaunchItem) -> String {
        match item {
            LaunchItem::DesktopApp(desktop_id) => desktop_id.clone(),
            LaunchItem::PathExecutable(name) => {
                // Remove "path:" prefix if present for frecency tracking
                name.strip_prefix("path:").unwrap_or(name).to_string()
            }
        }
    }

    /// Static version of save_frecency_sorted_cache for background processing
    fn save_frecency_sorted_cache_static(cache_path: PathBuf, items: &[LaunchItem]) -> Result<()> {
        let content: Vec<String> = items
            .iter()
            .map(|item| match item {
                LaunchItem::DesktopApp(id) => format!("desktop:{id}"),
                LaunchItem::PathExecutable(name) => format!("path:{name}"),
            })
            .collect();

        fs::write(cache_path, content.join("\n"))
            .context("Failed to write frecency sorted cache file")?;
        Ok(())
    }
}
