use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: Option<String>,
    pub branch: Option<String>,
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
            fs::write(&cfg_path, toml).with_context(|| format!("writing default config to {}", cfg_path.display()))?;
            return Ok(default);
        }
        let s = fs::read_to_string(&cfg_path).with_context(|| format!("reading config {}", cfg_path.display()))?;
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
}

pub fn repos_base_dir() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let base = PathBuf::from(home).join(".local/share/instantos/dots");
    fs::create_dir_all(&base).context("creating repos base directory")?;
    Ok(base)
}




