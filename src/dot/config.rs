use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: Option<String>,
    pub branch: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Config {
    pub repos: Vec<Repo>,
}

fn config_file_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let cfg = PathBuf::from(home).join(".config/instant/instant.toml");
    if let Some(parent) = cfg.parent() {
        fs::create_dir_all(parent).context("creating config directory")?;
    }
    Ok(cfg)
}

fn repos_base_dir() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let base = PathBuf::from(home).join(".local/share/instantos/dots");
    fs::create_dir_all(&base).context("creating repos base directory")?;
    Ok(base)
}

pub fn load_repos() -> Result<Vec<Repo>> {
    let cfg = config_file_path()?;
    if !cfg.exists() {
        return Ok(Vec::new());
    }
    let s = fs::read_to_string(&cfg).with_context(|| format!("reading config {}", cfg.display()))?;
    let c: Config = toml::from_str(&s).context("parsing config toml")?;
    Ok(c.repos)
}

pub fn save_repos(repos: &[Repo]) -> Result<()> {
    let cfg = config_file_path()?;
    let c = Config { repos: repos.to_vec() };
    let toml = toml::to_string_pretty(&c).context("serializing config to toml")?;
    fs::write(cfg, toml).context("writing config file")?;
    Ok(())
}

pub fn repos_base_dir_path() -> Result<PathBuf> {
    repos_base_dir()
}

pub fn config_file_path_str() -> Result<String> {
    Ok(config_file_path()?.to_string_lossy().to_string())
}
