use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: String,
    pub branch: Option<String>,
    #[serde(default = "default_active_subdirs")]
    pub active_subdirectories: Vec<String>,
}

fn default_active_subdirs() -> Vec<String> {
    // By default, only the first subdirectory is active
    vec!["dots".to_string()]
}

fn default_clone_depth() -> u32 {
    1
}

fn default_hash_cleanup_days() -> u32 {
    30
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default)]
    pub repos: Vec<Repo>,
    #[serde(default = "default_clone_depth")]
    pub clone_depth: u32,
    #[serde(default = "default_hash_cleanup_days")]
    pub hash_cleanup_days: u32,
    #[serde(default)]
    pub repos_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            repos: Vec::new(),
            clone_depth: default_clone_depth(),
            hash_cleanup_days: default_hash_cleanup_days(),
            repos_dir: None,
        }
    }
}

pub fn config_file_path(custom_path: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = custom_path {
        return Ok(PathBuf::from(path));
    }

    let config_dir = dirs::config_dir()
        .context("Unable to determine config directory")?
        .join("instant");

    fs::create_dir_all(&config_dir).context("creating config directory")?;
    Ok(config_dir.join("instant.toml"))
}

impl Config {
    /// Load the config from disk. If the config file does not exist,
    /// create a default config file and return the default.
    pub fn load() -> Result<Config> {
        Self::load_from(None)
    }

    /// Load config from a specific path or the default location
    pub fn load_from(custom_path: Option<&str>) -> Result<Config> {
        let cfg_path = config_file_path(custom_path)?;
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
        self.save_to(None)
    }

    /// Save config to a specific path or the default location
    pub fn save_to(&self, custom_path: Option<&str>) -> Result<()> {
        let cfg_path = config_file_path(custom_path)?;
        let toml = toml::to_string_pretty(self).context("serializing config to toml")?;
        fs::write(cfg_path, toml).context("writing config file")?;
        Ok(())
    }

    /// Add a repo to the config and persist the change
    pub fn add_repo(&mut self, mut repo: Repo) -> Result<()> {
        // Auto-generate name if not provided (though it's now mandatory in the struct)
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
        self.save()
    }

    /// Set active subdirectories for a specific repo by name
    pub fn set_active_subdirs(&mut self, repo_name: &str, subdirs: Vec<String>) -> Result<()> {
        for repo in &mut self.repos {
            if repo.name == repo_name {
                repo.active_subdirectories = subdirs;
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
            .map(|repo| {
                if repo.active_subdirectories.is_empty() {
                    default_active_subdirs()
                } else {
                    repo.active_subdirectories.clone()
                }
            })
            .unwrap_or_else(|| default_active_subdirs())
    }
}

/// Wrapper that holds config and its custom path
#[derive(Debug, Clone)]
pub struct ConfigManager {
    pub config: Config,
    pub custom_path: Option<String>,
}

impl ConfigManager {
    /// Load config from a specific path or the default location
    pub fn load_from(custom_path: Option<&str>) -> Result<Self> {
        let config = Config::load_from(custom_path)?;
        Ok(Self {
            config,
            custom_path: custom_path.map(|s| s.to_string()),
        })
    }

    /// Save the config to the original path it was loaded from
    pub fn save(&self) -> Result<()> {
        self.config.save_to(self.custom_path.as_deref())
    }

    /// Get a mutable reference to the config
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }
}

pub fn db_path(custom_path: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = custom_path {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("creating db directory")?;
        }
        return Ok(path);
    }

    let data_dir = dirs::data_dir()
        .context("Unable to determine data directory")?
        .join("instantos");

    fs::create_dir_all(&data_dir).context("creating db directory")?;
    Ok(data_dir.join("instant.db"))
}

pub fn repos_dir(custom_path: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = custom_path {
        let base = PathBuf::from(path);
        fs::create_dir_all(&base).context("creating repos directory")?;
        return Ok(base);
    }

    let data_dir = dirs::data_dir()
        .context("Unable to determine data directory")?
        .join("instantos")
        .join("dots");

    fs::create_dir_all(&data_dir).context("creating repos base directory")?;
    Ok(data_dir)
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
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
