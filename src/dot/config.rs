use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::common::paths;
use crate::dot::path_serde::TildePath;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: String,
    pub branch: Option<String>,
    #[serde(default = "default_active_subdirs")]
    pub active_subdirectories: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_active_subdirs() -> Vec<String> {
    // By default, only the first subdirectory is active
    vec!["dots".to_string()]
}

fn default_enabled() -> bool {
    true
}

fn default_clone_depth() -> u32 {
    1
}

fn default_hash_cleanup_days() -> u32 {
    30
}

fn default_repos_dir() -> TildePath {
    TildePath::new(paths::dots_repo_dir().unwrap_or_else(|_| {
        PathBuf::from("~/.local/share")
            .join("instant")
            .join("dots")
    }))
}

fn default_database_dir() -> TildePath {
    TildePath::new(paths::instant_data_dir().unwrap_or_else(|_| {
        PathBuf::from("~/.local/share")
            .join("instant")
    }).join("instant.db"))
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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            repos: Vec::new(),
            clone_depth: default_clone_depth(),
            hash_cleanup_days: default_hash_cleanup_days(),
            repos_dir: default_repos_dir(),
            database_dir: default_database_dir(),
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
            .map(|repo| {
                if repo.active_subdirectories.is_empty() {
                    default_active_subdirs()
                } else {
                    repo.active_subdirectories.clone()
                }
            })
            .unwrap_or_else(default_active_subdirs)
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
        let config = if !cfg_path.exists() {
            let default = Config::default();
            let toml = toml::to_string_pretty(&default).context("serializing default config")?;
            fs::write(&cfg_path, toml)
                .with_context(|| format!("writing default config to {}", cfg_path.display()))?;
            default
        } else {
            let s = fs::read_to_string(&cfg_path)
                .with_context(|| format!("reading config {}", cfg_path.display()))?;
            toml::from_str(&s).context("parsing config toml")?
        };

        Ok(config)
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
                repo.active_subdirectories = subdirs;
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
