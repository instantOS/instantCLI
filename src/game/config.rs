use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::dot::path_serde::TildePath;

/// Wrapper type for game names
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameName(pub String);

impl std::fmt::Display for GameName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for GameName {
    fn from(s: String) -> Self {
        GameName(s)
    }
}

impl From<&str> for GameName {
    fn from(s: &str) -> Self {
        GameName(s.to_string())
    }
}

/// Game configuration - shared across devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub name: GameName,
    pub description: Option<String>,
    pub launch_command: Option<String>,
}

impl Game {
    pub fn new(name: impl Into<GameName>) -> Self {
        Self {
            name: name.into(),
            description: None,
            launch_command: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_launch_command(mut self, command: impl Into<String>) -> Self {
        self.launch_command = Some(command.into());
        self
    }
}

/// Game installation - device-specific
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInstallation {
    pub game_name: GameName,
    pub save_path: TildePath,
}

impl GameInstallation {
    pub fn new(game_name: impl Into<GameName>, save_path: impl Into<TildePath>) -> Self {
        Self {
            game_name: game_name.into(),
            save_path: save_path.into(),
        }
    }
}

/// Main game configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstantGameConfig {
    pub repo: TildePath,
    pub repo_password: String,
    pub games: Vec<Game>,
}

impl Default for InstantGameConfig {
    fn default() -> Self {
        Self {
            repo: TildePath::new(PathBuf::new()),
            repo_password: "instantgamepassword".to_string(),
            games: Vec::new(),
        }
    }
}

/// Device-specific installations configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct InstallationsConfig {
    pub installations: Vec<GameInstallation>,
}


pub fn games_config_dir() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Unable to determine config directory")?
        .join("instant")
        .join("games");

    fs::create_dir_all(&config_dir).context("creating games config directory")?;
    Ok(config_dir)
}

pub fn games_config_path() -> Result<PathBuf> {
    Ok(games_config_dir()?.join("games.toml"))
}

pub fn installations_config_path() -> Result<PathBuf> {
    Ok(games_config_dir()?.join("installations.toml"))
}

impl InstantGameConfig {
    pub fn load() -> Result<Self> {
        Self::load_from_path(games_config_path()?)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let content = fs::read_to_string(path).context("reading games config")?;
        let config: Self = toml::from_str(&content).context("parsing games config")?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(games_config_path()?)
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let content = toml::to_string_pretty(self).context("serializing games config")?;
        fs::write(path, content).context("writing games config")?;
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        !self.repo.as_path().as_os_str().is_empty()
    }
}

impl InstallationsConfig {
    pub fn load() -> Result<Self> {
        Self::load_from_path(installations_config_path()?)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let content = fs::read_to_string(path).context("reading installations config")?;
        let config: Self = toml::from_str(&content).context("parsing installations config")?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(installations_config_path()?)
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let content = toml::to_string_pretty(self).context("serializing installations config")?;
        fs::write(path, content).context("writing installations config")?;
        Ok(())
    }
}
