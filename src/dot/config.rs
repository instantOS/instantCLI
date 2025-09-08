use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: Option<String>,
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
    pub fn add_repo(&mut self, repo: Repo) -> Result<()> {
        self.repos.push(repo);
        self.save()
    }

    /// Set active subdirectories for a specific repo by URL
    pub fn set_active_subdirs(&mut self, repo_url: &str, subdirs: Vec<String>) -> Result<()> {
        for repo in &mut self.repos {
            if repo.url == repo_url {
                repo.active_subdirs = subdirs;
                return self.save();
            }
        }
        Err(anyhow::anyhow!("Repository with URL '{}' not found", repo_url))
    }

    /// Get active subdirectories for a specific repo by URL
    pub fn get_active_subdirs(&self, repo_url: &str) -> Option<Vec<String>> {
        self.repos.iter()
            .find(|repo| repo.url == repo_url)
            .map(|repo| repo.active_subdirs.clone())
    }
    
    // TODO make name not url the identifier used for repos. makee name  mandatory. when adding
    // repos, get the name automatically, but  do add it
    // remove the variants which use urlbfor identifying repos. when adding repos, make sure no two
    // repos have the same name

    /// Get active subdirectories for a repo by its local name
    pub fn get_active_subdirs_by_name(&self, repo_name: &str) -> Option<Vec<String>> {
        self.repos.iter()
            .find(|repo| repo.name.as_deref() == Some(repo_name) || repo.name.as_deref().unwrap_or("") == repo_name)
            .map(|repo| repo.active_subdirs.clone())
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

pub fn repos_base_dir() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let base = PathBuf::from(home).join(".local/share/instantos/dots");
    fs::create_dir_all(&base).context("creating repos base directory")?;
    Ok(base)
}

pub fn basename_from_repo(repo: &str) -> String {
    let s = repo.trim_end_matches(".git");
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
