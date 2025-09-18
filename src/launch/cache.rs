use anyhow::{Context, Result};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::task;

/// Application launcher cache for fast startup with background refresh
pub struct LaunchCache {
    cache_path: PathBuf,
}

impl LaunchCache {
    /// Create a new launch cache instance
    pub fn new() -> Result<Self> {
        let cache_dir = if let Some(cache_dir) = dirs::cache_dir() {
            cache_dir.join("instant")
        } else {
            PathBuf::from(env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())).join(".cache/instant")
        };
        
        // Ensure cache directory exists
        fs::create_dir_all(&cache_dir)
            .context("Failed to create cache directory")?;
        
        let cache_path = cache_dir.join("launch_cache");
        
        Ok(Self { cache_path })
    }
    
    /// Get applications with dmenu-style caching strategy
    pub async fn get_applications(&mut self) -> Result<Vec<String>> {
        // Check if cache is fresh
        if self.is_cache_fresh()? {
            // Use fresh cache
            self.read_cache()
        } else {
            // Cache is stale or doesn't exist
            let stale_apps = self.read_cache().unwrap_or_default();
            
            // Spawn background task to refresh cache
            let cache_path = self.cache_path.clone();
            task::spawn(async move {
                if let Err(e) = Self::refresh_cache_background(cache_path).await {
                    eprintln!("Warning: Failed to refresh application cache: {}", e);
                }
            });
            
            // Return stale cache immediately for fast startup
            if stale_apps.is_empty() {
                // If no stale cache, do a quick scan now
                self.scan_path_directories()
            } else {
                Ok(stale_apps)
            }
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
        
        let content = fs::read_to_string(&self.cache_path)
            .context("Failed to read cache file")?;
        
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
        fs::write(cache_path, content)
            .context("Failed to write cache file")?;
        Ok(())
    }
}
