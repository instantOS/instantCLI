use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: String,
    pub branch: Option<String>,
    #[serde(default = "default_active_subdirs")]
    pub active_subdirs: Vec<String>,
}

fn default_active_subdirs() -> Vec<String> {
    // By default, only the first subdirectory is active
    vec!["dots".to_string()]
}

fn default_clone_depth() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default)]
    pub repos: Vec<Repo>,
    #[serde(default = "default_clone_depth")]
    pub clone_depth: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            repos: Vec::new(),
            clone_depth: default_clone_depth(),
        }
    }
}

fn config_file_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let cfg = PathBuf::from(home).join(".config/instant/instant.toml");
    if let Some(parent) = cfg.parent() {
        fs::create_dir_all(parent).context("creating config directory")?;
    }
    Ok(cfg)
}

impl Config {
    /// Load the config from disk. If the config file does not exist,
    /// create a default config file and return the default.
    pub fn load() -> Result<Config> {
        let cfg_path = config_file_path()?;
        if !cfg_path.exists() {
            let default = Config::default();
            let toml = toml::to_string_pretty(&default).context("serializing default config")?;
            fs::write(&cfg_path, toml)
                .with_context(|| format!("writing default config to {}", cfg_path.display()))?;
            return Ok(default);
        }
        let s = fs::read_to_string(&cfg_path)
            .with_context(|| format!("reading config {}", cfg_path.display()))?;
        let c: Config = toml::from_str(&s).context("parsing config toml")?;
        Ok(c)
    }

    /// Save the current config to disk (overwrites file)
    pub fn save(&self) -> Result<()> {
        let cfg_path = config_file_path()?;
        let toml = toml::to_string_pretty(self).context("serializing config to toml")?;
        fs::write(cfg_path, toml).context("writing config file")?;
        Ok(())
    }

    /// Add a repo to the config and persist the change
    pub fn add_repo(&mut self, mut repo: Repo) -> Result<()> {
        // Auto-generate name if not provided (though it's now mandatory in the struct)
        if repo.name.trim().is_empty() {
            repo.name = basename_from_repo(&repo.url);
        }

        // Check for duplicate names
        if self.repos.iter().any(|r| r.name == repo.name) {
            return Err(anyhow::anyhow!(
                "Repository with name '{}' already exists",
                repo.name
            ));
        }

        self.repos.push(repo);
        self.save()
    }

    /// Set active subdirectories for a specific repo by name
    pub fn set_active_subdirs(&mut self, repo_name: &str, subdirs: Vec<String>) -> Result<()> {
        for repo in &mut self.repos {
            if repo.name == repo_name {
                repo.active_subdirs = subdirs;
                return self.save();
            }
        }
        Err(anyhow::anyhow!(
            "Repository with name '{}' not found",
            repo_name
        ))
    }

    /// Get active subdirectories for a specific repo by name
    pub fn get_active_subdirs(&self, repo_name: &str) -> Vec<String> {
        self.repos
            .iter()
            .find(|repo| repo.name == repo_name)
            .map(|repo| repo.active_subdirs.clone())
            .unwrap_or_else(|| vec!["dots".to_string()])
    }
}

pub fn db_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let path = PathBuf::from(home).join(".local/share/instantos/instant.db");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("creating db directory")?;
    }
    Ok(path)
}

pub fn repos_dir() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let base = PathBuf::from(home).join(".local/share/instantos/dots");
    fs::create_dir_all(&base).context("creating repos base directory")?;
    Ok(base)
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
/// let name = basename_from_repo("https://github.com/user/my-repo.git");
/// assert_eq!(name, "my-repo");
///
/// let name = basename_from_repo("git@github.com:user/dotfiles");
/// assert_eq!(name, "dotfiles");
/// ```
pub fn basename_from_repo(repo: &str) -> String {
    let s = repo.trim_end_matches(".git");
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
