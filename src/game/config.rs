use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::dot::path_serde::TildePath;

/// Configurable restic retention policy values stored in games.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetentionPolicyConfig {
    pub keep_last: Option<u32>,
    pub keep_daily: Option<u32>,
    pub keep_weekly: Option<u32>,
    pub keep_monthly: Option<u32>,
    pub keep_yearly: Option<u32>,
}

impl RetentionPolicyConfig {
    pub const DEFAULT_KEEP_LAST: u32 = 30;
    pub const DEFAULT_KEEP_DAILY: u32 = 90;
    pub const DEFAULT_KEEP_WEEKLY: u32 = 52;
    pub const DEFAULT_KEEP_MONTHLY: u32 = 36;
    pub const DEFAULT_KEEP_YEARLY: u32 = 10;

    pub fn is_default(value: &Self) -> bool {
        value.keep_last.is_none()
            && value.keep_daily.is_none()
            && value.keep_weekly.is_none()
            && value.keep_monthly.is_none()
            && value.keep_yearly.is_none()
    }

    pub fn effective(&self) -> RetentionPolicyValues {
        RetentionPolicyValues {
            keep_last: self.keep_last.unwrap_or(Self::DEFAULT_KEEP_LAST),
            keep_daily: self.keep_daily.unwrap_or(Self::DEFAULT_KEEP_DAILY),
            keep_weekly: self.keep_weekly.unwrap_or(Self::DEFAULT_KEEP_WEEKLY),
            keep_monthly: self.keep_monthly.unwrap_or(Self::DEFAULT_KEEP_MONTHLY),
            keep_yearly: self.keep_yearly.unwrap_or(Self::DEFAULT_KEEP_YEARLY),
        }
    }
}

impl Default for RetentionPolicyConfig {
    fn default() -> Self {
        Self {
            keep_last: None,
            keep_daily: None,
            keep_weekly: None,
            keep_monthly: None,
            keep_yearly: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicyValues {
    pub keep_last: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
    pub keep_yearly: u32,
}

impl RetentionPolicyValues {
    pub fn to_rules(&self) -> Vec<(String, String)> {
        vec![
            ("keep-last".to_string(), self.keep_last.to_string()),
            ("keep-daily".to_string(), self.keep_daily.to_string()),
            ("keep-weekly".to_string(), self.keep_weekly.to_string()),
            ("keep-monthly".to_string(), self.keep_monthly.to_string()),
            ("keep-yearly".to_string(), self.keep_yearly.to_string()),
        ]
    }
}

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
    pub nearest_checkpoint: Option<String>,
}

impl GameInstallation {
    pub fn new(game_name: impl Into<GameName>, save_path: impl Into<TildePath>) -> Self {
        Self {
            game_name: game_name.into(),
            save_path: save_path.into(),
            nearest_checkpoint: None,
        }
    }

    pub fn with_checkpoint(mut self, checkpoint_id: impl Into<String>) -> Self {
        self.nearest_checkpoint = Some(checkpoint_id.into());
        self
    }

    pub fn update_checkpoint(&mut self, checkpoint_id: impl Into<String>) {
        self.nearest_checkpoint = Some(checkpoint_id.into());
    }

    pub fn clear_checkpoint(&mut self) {
        self.nearest_checkpoint = None;
    }
}

/// Main game configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstantGameConfig {
    pub repo: TildePath,
    pub repo_password: String,
    pub games: Vec<Game>,
    #[serde(default, skip_serializing_if = "RetentionPolicyConfig::is_default")]
    pub retention_policy: RetentionPolicyConfig,
}

impl Default for InstantGameConfig {
    fn default() -> Self {
        Self {
            repo: TildePath::new(PathBuf::new()),
            repo_password: "instantgamepassword".to_string(),
            games: Vec::new(),
            retention_policy: RetentionPolicyConfig::default(),
        }
    }
}

/// Device-specific installations configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_game_installation_new() {
        let installation =
            GameInstallation::new("test_game", TildePath::new(PathBuf::from("~/.test/saves")));

        assert_eq!(installation.game_name.0, "test_game");
        assert_eq!(installation.nearest_checkpoint, None);
    }

    #[test]
    fn test_game_installation_with_checkpoint() {
        let installation =
            GameInstallation::new("test_game", TildePath::new(PathBuf::from("~/.test/saves")))
                .with_checkpoint("checkpoint123");

        assert_eq!(installation.game_name.0, "test_game");
        assert_eq!(
            installation.nearest_checkpoint,
            Some("checkpoint123".to_string())
        );
    }

    #[test]
    fn test_game_installation_update_checkpoint() {
        let mut installation =
            GameInstallation::new("test_game", TildePath::new(PathBuf::from("~/.test/saves")));

        installation.update_checkpoint("checkpoint456");
        assert_eq!(
            installation.nearest_checkpoint,
            Some("checkpoint456".to_string())
        );

        installation.update_checkpoint("checkpoint789");
        assert_eq!(
            installation.nearest_checkpoint,
            Some("checkpoint789".to_string())
        );
    }

    #[test]
    fn test_game_installation_clear_checkpoint() {
        let mut installation =
            GameInstallation::new("test_game", TildePath::new(PathBuf::from("~/.test/saves")))
                .with_checkpoint("checkpoint123");

        assert_eq!(
            installation.nearest_checkpoint,
            Some("checkpoint123".to_string())
        );

        installation.clear_checkpoint();
        assert_eq!(installation.nearest_checkpoint, None);
    }

    #[test]
    fn test_retention_policy_defaults() {
        let policy = RetentionPolicyConfig::default();
        let effective = policy.effective();

        assert_eq!(
            effective.keep_last,
            RetentionPolicyConfig::DEFAULT_KEEP_LAST
        );
        assert_eq!(
            effective.keep_daily,
            RetentionPolicyConfig::DEFAULT_KEEP_DAILY
        );
        assert_eq!(
            effective.keep_weekly,
            RetentionPolicyConfig::DEFAULT_KEEP_WEEKLY
        );
        assert_eq!(
            effective.keep_monthly,
            RetentionPolicyConfig::DEFAULT_KEEP_MONTHLY
        );
        assert_eq!(
            effective.keep_yearly,
            RetentionPolicyConfig::DEFAULT_KEEP_YEARLY
        );
    }

    #[test]
    fn test_retention_policy_overrides() {
        let policy = RetentionPolicyConfig {
            keep_last: Some(5),
            keep_daily: Some(7),
            keep_weekly: Some(9),
            keep_monthly: Some(11),
            keep_yearly: Some(13),
        };

        let effective = policy.effective();

        assert_eq!(effective.keep_last, 5);
        assert_eq!(effective.keep_daily, 7);
        assert_eq!(effective.keep_weekly, 9);
        assert_eq!(effective.keep_monthly, 11);
        assert_eq!(effective.keep_yearly, 13);

        let rules = effective.to_rules();
        assert_eq!(rules.len(), 5);
        assert!(rules.contains(&("keep-last".to_string(), "5".to_string())));
        assert!(rules.contains(&("keep-yearly".to_string(), "13".to_string())));
    }
}
