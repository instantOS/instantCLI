use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::common::TildePath;
use crate::common::config::DocumentedConfig;
use crate::common::paths;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: String,
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_subdirectories: Option<Vec<String>>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_read_only")]
    pub read_only: bool,
    /// Optional metadata for repositories that don't have an instantdots.toml file (e.g. yadm/stow)
    pub metadata: Option<crate::dot::types::RepoMetaData>,
}

fn default_enabled() -> bool {
    true
}

fn default_read_only() -> bool {
    false
}

fn default_clone_depth() -> u32 {
    1
}

fn default_hash_cleanup_days() -> u32 {
    30
}

fn default_repos_dir() -> TildePath {
    TildePath::new(
        paths::dots_repo_dir()
            .unwrap_or_else(|_| PathBuf::from("~/.local/share").join("instant").join("dots")),
    )
}

fn default_database_dir() -> TildePath {
    TildePath::new(
        paths::instant_data_dir()
            .unwrap_or_else(|_| PathBuf::from("~/.local/share").join("instant"))
            .join("instant.db"),
    )
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default)]
    pub repos: Vec<Repo>,
    #[serde(default = "default_clone_depth")]
    pub clone_depth: u32,
    #[serde(default = "default_hash_cleanup_days")]
    pub hash_cleanup_days: u32,
    #[serde(default = "default_repos_dir")]
    pub repos_dir: TildePath,
    #[serde(default = "default_database_dir")]
    pub database_dir: TildePath,
    #[serde(default)]
    pub ignored_paths: Vec<String>,
    /// Global dotfile units - directories treated as atomic.
    /// Combined with per-repo units from instantdots.toml.
    #[serde(default)]
    pub units: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            repos: Vec::new(),
            clone_depth: default_clone_depth(),
            hash_cleanup_days: default_hash_cleanup_days(),
            repos_dir: default_repos_dir(),
            database_dir: default_database_dir(),
            ignored_paths: Vec::new(),
            units: Vec::new(),
        }
    }
}

pub fn config_file_path(custom_path: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = custom_path {
        return Ok(PathBuf::from(path));
    }

    Ok(paths::instant_config_dir()?.join("dots.toml"))
}

impl Config {
    /// Get active subdirectories for a specific repo by name
    pub fn get_active_subdirs(&self, repo_name: &str) -> Vec<String> {
        self.repos
            .iter()
            .find(|repo| repo.name == repo_name)
            .map(|repo| self.resolve_active_subdirs(repo))
            .unwrap_or_default()
    }

    pub fn resolve_active_subdirs(&self, repo: &Repo) -> Vec<String> {
        if let Some(subdirs) = &repo.active_subdirectories {
            return subdirs.clone();
        }

        let repo_path = self.repos_path().join(&repo.name);
        let meta = if let Some(meta) = &repo.metadata {
            Some(meta.clone())
        } else if repo_path.join("instantdots.toml").exists() {
            crate::dot::meta::read_meta(&repo_path).ok()
        } else {
            None
        };

        match meta {
            Some(meta) => self.resolve_active_subdirs_from_meta(&repo_path, &meta),
            None => self.detect_repo_subdirs(&repo_path),
        }
    }

    fn resolve_active_subdirs_from_meta(
        &self,
        repo_path: &Path,
        meta: &crate::dot::types::RepoMetaData,
    ) -> Vec<String> {
        if let Some(default_active) = meta.default_active_subdirs.as_ref() {
            return default_active
                .iter()
                .filter(|dir| meta.dots_dirs.contains(*dir))
                .filter(|dir| repo_path.join(dir).is_dir())
                .cloned()
                .collect();
        }

        // Only activate the first directory in dots_dirs by default
        meta.dots_dirs
            .first()
            .into_iter()
            .filter(|dir| repo_path.join(dir).is_dir())
            .cloned()
            .collect()
    }

    fn detect_repo_subdirs(&self, repo_path: &Path) -> Vec<String> {
        let mut subdirs = Vec::new();

        if let Ok(entries) = fs::read_dir(repo_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                if name == ".git" {
                    continue;
                }

                subdirs.push(name.to_string());
            }

            subdirs.sort();
            subdirs.dedup();
        }

        subdirs
    }
    /// Get all writable repositories
    pub fn get_writable_repos(&self) -> Vec<&Repo> {
        self.repos.iter().filter(|r| !r.read_only).collect()
    }

    /// Get the database path as a PathBuf
    pub fn database_path(&self) -> &Path {
        self.database_dir.as_path()
    }

    /// Get the repos directory as a PathBuf
    pub fn repos_path(&self) -> &Path {
        self.repos_dir.as_path()
    }

    /// Ensure all directory paths exist
    pub fn ensure_directories(&self) -> Result<()> {
        if let Some(parent) = self.database_path().parent() {
            fs::create_dir_all(parent).context("creating database directory")?;
        }

        fs::create_dir_all(self.repos_path()).context("creating repos directory")?;
        Ok(())
    }

    /// Load config from default location or custom path
    pub fn load(custom_path: Option<&str>) -> Result<Self> {
        let cfg_path = config_file_path(custom_path)?;
        <Self as DocumentedConfig>::load_from_path_documented(cfg_path)
    }

    /// Save the config to default location or custom path
    pub fn save(&self, custom_path: Option<&str>) -> Result<()> {
        let cfg_path = config_file_path(custom_path)?;
        let toml = toml::to_string_pretty(self).context("serializing config to toml")?;
        fs::write(cfg_path, toml).context("writing config file")?;
        Ok(())
    }

    /// Add a repo to the config and persist the change
    pub fn add_repo(&mut self, mut repo: Repo, custom_path: Option<&str>) -> Result<()> {
        // Auto-generate name if not provided
        if repo.name.trim().is_empty() {
            repo.name = extract_repo_name(&repo.url);
        }

        // Check for duplicate names
        if self.repos.iter().any(|r| r.name == repo.name) {
            return Err(anyhow::anyhow!(
                "Repository with name '{}' already exists",
                repo.name
            ));
        }

        self.repos.push(repo);
        self.save(custom_path)
    }

    /// Set active subdirectories for a specific repo by name
    pub fn set_active_subdirs(
        &mut self,
        repo_name: &str,
        subdirs: Vec<String>,
        custom_path: Option<&str>,
    ) -> Result<()> {
        for repo in &mut self.repos {
            if repo.name == repo_name {
                repo.active_subdirectories = Some(subdirs);
                return self.save(custom_path);
            }
        }
        Err(anyhow::anyhow!(
            "Repository with name '{}' not found",
            repo_name
        ))
    }

    /// Enable a repository by name
    pub fn enable_repo(&mut self, repo_name: &str, custom_path: Option<&str>) -> Result<()> {
        for repo in &mut self.repos {
            if repo.name == repo_name {
                repo.enabled = true;
                return self.save(custom_path);
            }
        }
        Err(anyhow::anyhow!(
            "Repository with name '{}' not found",
            repo_name
        ))
    }

    /// Disable a repository by name
    pub fn disable_repo(&mut self, repo_name: &str, custom_path: Option<&str>) -> Result<()> {
        for repo in &mut self.repos {
            if repo.name == repo_name {
                repo.enabled = false;
                return self.save(custom_path);
            }
        }
        Err(anyhow::anyhow!(
            "Repository with name '{}' not found",
            repo_name
        ))
    }

    /// Remove a repository by name
    pub fn remove_repo(&mut self, repo_name: &str, custom_path: Option<&str>) -> Result<()> {
        let original_len = self.repos.len();
        self.repos.retain(|r| r.name != repo_name);
        if self.repos.len() == original_len {
            return Err(anyhow::anyhow!(
                "Repository with name '{}' not found",
                repo_name
            ));
        }
        self.save(custom_path)
    }

    /// Add a path to the ignore list
    pub fn add_ignored_path(&mut self, path: String, custom_path: Option<&str>) -> Result<()> {
        if self.ignored_paths.contains(&path) {
            return Err(anyhow::anyhow!("Path '{}' is already ignored", path));
        }
        self.ignored_paths.push(path);
        self.save(custom_path)
    }

    /// Remove a path from the ignore list
    pub fn remove_ignored_path(&mut self, path: &str, custom_path: Option<&str>) -> Result<()> {
        let original_len = self.ignored_paths.len();
        self.ignored_paths.retain(|p| p != path);
        if self.ignored_paths.len() == original_len {
            return Err(anyhow::anyhow!("Path '{}' is not in the ignore list", path));
        }
        self.save(custom_path)
    }

    /// Check if a path should be ignored
    pub fn is_path_ignored(&self, path: &Path) -> bool {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());

        for ignored in &self.ignored_paths {
            let ignored_path = if ignored.starts_with('~') {
                PathBuf::from(shellexpand::tilde(ignored).to_string())
            } else {
                home.join(ignored)
            };

            // Check if the path starts with the ignored path (for directories)
            // or is exactly the ignored path (for files)
            if path == ignored_path || path.starts_with(&ignored_path) {
                return true;
            }
        }

        false
    }

    /// Add a directory to the global units list
    pub fn add_unit(&mut self, path: String, custom_path: Option<&str>) -> Result<()> {
        if self.units.contains(&path) {
            return Err(anyhow::anyhow!("Path '{}' is already a unit", path));
        }
        self.units.push(path);
        self.save(custom_path)
    }

    /// Remove a directory from the global units list
    pub fn remove_unit(&mut self, path: &str, custom_path: Option<&str>) -> Result<()> {
        let original_len = self.units.len();
        self.units.retain(|p| p != path);
        if self.units.len() == original_len {
            return Err(anyhow::anyhow!("Path '{}' is not in the units list", path));
        }
        self.save(custom_path)
    }

    /// Move a repository up in priority (earlier in list = higher priority)
    /// Returns the new priority position (1-indexed)
    pub fn move_repo_up(&mut self, repo_name: &str, custom_path: Option<&str>) -> Result<usize> {
        let index = self
            .repos
            .iter()
            .position(|r| r.name == repo_name)
            .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", repo_name))?;

        if index == 0 {
            return Err(anyhow::anyhow!(
                "Repository '{}' is already at highest priority",
                repo_name
            ));
        }

        self.repos.swap(index, index - 1);
        self.save(custom_path)?;
        Ok(index) // New position (1-indexed: was index+1, now index)
    }

    /// Move a repository down in priority (later in list = lower priority)
    /// Returns the new priority position (1-indexed)
    pub fn move_repo_down(&mut self, repo_name: &str, custom_path: Option<&str>) -> Result<usize> {
        let index = self
            .repos
            .iter()
            .position(|r| r.name == repo_name)
            .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", repo_name))?;

        if index >= self.repos.len() - 1 {
            return Err(anyhow::anyhow!(
                "Repository '{}' is already at lowest priority",
                repo_name
            ));
        }

        self.repos.swap(index, index + 1);
        self.save(custom_path)?;
        Ok(index + 2) // New position (1-indexed: was index+1, now index+2)
    }

    /// Set a repository to a specific priority position (1-indexed)
    /// Position 1 = highest priority (first in list)
    pub fn set_repo_priority(
        &mut self,
        repo_name: &str,
        position: usize,
        custom_path: Option<&str>,
    ) -> Result<()> {
        if position == 0 || position > self.repos.len() {
            return Err(anyhow::anyhow!(
                "Invalid priority position {}. Must be between 1 and {}",
                position,
                self.repos.len()
            ));
        }

        let current_index = self
            .repos
            .iter()
            .position(|r| r.name == repo_name)
            .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", repo_name))?;

        let target_index = position - 1;

        if current_index == target_index {
            return Ok(()); // Already at target position
        }

        let repo = self.repos.remove(current_index);
        self.repos.insert(target_index, repo);
        self.save(custom_path)
    }

    /// Move a subdirectory up in priority within a repository (earlier in list = higher priority)
    /// Returns the new priority position (1-indexed)
    pub fn move_subdir_up(
        &mut self,
        repo_name: &str,
        subdir_name: &str,
        custom_path: Option<&str>,
    ) -> Result<usize> {
        let repo = self
            .repos
            .iter_mut()
            .find(|r| r.name == repo_name)
            .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", repo_name))?;

        let active_subdirs = repo.active_subdirectories.as_mut().ok_or_else(|| {
            anyhow::anyhow!(
                "Subdirectory '{}' is not configured in dots.toml for '{}'",
                subdir_name,
                repo_name
            )
        })?;

        let index = active_subdirs
            .iter()
            .position(|s| s == subdir_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Subdirectory '{}' is not configured in dots.toml for '{}'",
                    subdir_name,
                    repo_name
                )
            })?;

        if index == 0 {
            return Err(anyhow::anyhow!(
                "Subdirectory '{}' is already at highest priority",
                subdir_name
            ));
        }

        active_subdirs.swap(index, index - 1);
        self.save(custom_path)?;
        Ok(index) // New position (1-indexed: was index+1, now index)
    }

    /// Move a subdirectory down in priority within a repository (later in list = lower priority)
    /// Returns the new priority position (1-indexed)
    pub fn move_subdir_down(
        &mut self,
        repo_name: &str,
        subdir_name: &str,
        custom_path: Option<&str>,
    ) -> Result<usize> {
        let repo = self
            .repos
            .iter_mut()
            .find(|r| r.name == repo_name)
            .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", repo_name))?;

        let active_subdirs = repo.active_subdirectories.as_mut().ok_or_else(|| {
            anyhow::anyhow!(
                "Subdirectory '{}' is not configured in dots.toml for '{}'",
                subdir_name,
                repo_name
            )
        })?;

        let index = active_subdirs
            .iter()
            .position(|s| s == subdir_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Subdirectory '{}' is not configured in dots.toml for '{}'",
                    subdir_name,
                    repo_name
                )
            })?;

        if index >= active_subdirs.len() - 1 {
            return Err(anyhow::anyhow!(
                "Subdirectory '{}' is already at lowest priority",
                subdir_name
            ));
        }

        active_subdirs.swap(index, index + 1);
        self.save(custom_path)?;
        Ok(index + 2) // New position (1-indexed: was index+1, now index+2)
    }
}

/// Extract a repository name from a git URL by removing the .git suffix
/// and splitting on path separators and colons to get the last component.
///
/// # Arguments
/// * `repo` - The git repository URL or path
///
/// # Returns
/// The extracted repository name as a String
///
/// # Examples
/// ```
/// let name = extract_repo_name("https://github.com/user/my-repo.git");
/// assert_eq!(name, "my-repo");
///
/// let name = extract_repo_name("git@github.com:user/dotfiles");
/// assert_eq!(name, "dotfiles");
/// ```
pub fn extract_repo_name(repo: &str) -> String {
    let s = repo.trim_end_matches(".git");
    s.rsplit(['/', ':'])
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}

// Import macro from crate root
use crate::documented_config;

// Implement DocumentedConfig trait for Config using the macro
documented_config!(Config,
    clone_depth, "Git clone depth for repositories (default: 1 for shallow clones)",
    hash_cleanup_days, "Days before old file hashes are cleaned up from database (default: 30)",
    repos_dir, "Directory where dotfile repositories are stored",
    database_dir, "Path to the SQLite database storing file hashes",
    repos, "List of dotfile repositories to manage",
    ignored_paths, "Paths to ignore during dotfile operations (local overrides)",
    units, "Global dotfile units - directories treated as atomic (combined with per-repo units)",
    => config_file_path(None)
);
