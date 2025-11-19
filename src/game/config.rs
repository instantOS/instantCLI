use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::common::paths;
use crate::dot::path_serde::TildePath;

/// Describes what kind of filesystem element a tracked path represents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PathContentKind {
    #[default]
    Directory,
    File,
}

impl PathContentKind {
    pub fn is_file(self) -> bool {
        matches!(self, PathContentKind::File)
    }

    pub fn is_directory(self) -> bool {
        matches!(self, PathContentKind::Directory)
    }
}

impl From<std::fs::Metadata> for PathContentKind {
    fn from(metadata: std::fs::Metadata) -> Self {
        if metadata.is_file() {
            PathContentKind::File
        } else {
            PathContentKind::Directory
        }
    }
}

/// Configurable restic retention policy values stored in games.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct RetentionPolicyConfig {
    pub keep_last: Option<u32>,
    pub keep_daily: Option<u32>,
    pub keep_weekly: Option<u32>,
    pub keep_monthly: Option<u32>,
    pub keep_yearly: Option<u32>,
}

impl RetentionPolicyConfig {
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
            keep_last: self.keep_last,
            keep_daily: self.keep_daily.unwrap_or(Self::DEFAULT_KEEP_DAILY),
            keep_weekly: self.keep_weekly.unwrap_or(Self::DEFAULT_KEEP_WEEKLY),
            keep_monthly: self.keep_monthly.unwrap_or(Self::DEFAULT_KEEP_MONTHLY),
            keep_yearly: self.keep_yearly.unwrap_or(Self::DEFAULT_KEEP_YEARLY),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicyValues {
    pub keep_last: Option<u32>,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
    pub keep_yearly: u32,
}

impl RetentionPolicyValues {
    pub fn to_rules(&self) -> Vec<(String, String)> {
        let mut rules = Vec::with_capacity(5);

        if let Some(keep_last) = self.keep_last {
            rules.push(("keep-last".to_string(), keep_last.to_string()));
        }

        rules.push(("keep-daily".to_string(), self.keep_daily.to_string()));
        rules.push(("keep-weekly".to_string(), self.keep_weekly.to_string()));
        rules.push(("keep-monthly".to_string(), self.keep_monthly.to_string()));
        rules.push(("keep-yearly".to_string(), self.keep_yearly.to_string()));

        rules
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<GameDependency>,
}

impl Game {
    pub fn new(name: impl Into<GameName>) -> Self {
        Self {
            name: name.into(),
            description: None,
            launch_command: None,
            dependencies: Vec::new(),
        }
    }
}

/// Definition of a game dependency stored in games.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDependency {
    pub id: String,
    pub source_path: String,
    #[serde(default)]
    pub source_type: PathContentKind,
}

/// Game installation - device-specific
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInstallation {
    pub game_name: GameName,
    pub save_path: TildePath,
    #[serde(default)]
    pub save_path_type: PathContentKind,
    pub nearest_checkpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<InstalledDependency>,
}

impl GameInstallation {
    pub fn new(game_name: impl Into<GameName>, save_path: impl Into<TildePath>) -> Self {
        Self::with_kind(game_name, save_path, PathContentKind::Directory)
    }

    pub fn with_kind(
        game_name: impl Into<GameName>,
        save_path: impl Into<TildePath>,
        kind: PathContentKind,
    ) -> Self {
        Self {
            game_name: game_name.into(),
            save_path: save_path.into(),
            save_path_type: kind,
            nearest_checkpoint: None,
            launch_command: None,
            dependencies: Vec::new(),
        }
    }

    pub fn update_checkpoint(&mut self, checkpoint_id: impl Into<String>) {
        self.nearest_checkpoint = Some(checkpoint_id.into());
    }
}

/// Installed dependency mapping stored in installations.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledDependency {
    pub dependency_id: String,
    pub install_path: TildePath,
    #[serde(default)]
    pub install_path_type: PathContentKind,
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
    paths::games_config_dir()
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
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for game in &self.games {
            if !names.insert(&game.name.0) {
                return Err(anyhow::anyhow!(
                    "Duplicate game name found: {}",
                    game.name.0
                ));
            }
        }
        Ok(())
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
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for installation in &self.installations {
            if !names.insert(&installation.game_name.0) {
                return Err(anyhow::anyhow!(
                    "Duplicate installation for game found: {}",
                    installation.game_name.0
                ));
            }
        }
        Ok(())
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
    fn test_game_installation_update_checkpoint() {
        let mut installation = GameInstallation::new(
            GameName("test_game".to_string()),
            TildePath::new(PathBuf::from("~/.test/saves")),
        );

        installation.update_checkpoint("checkpoint456");
        assert_eq!(
            installation.nearest_checkpoint,
            Some("checkpoint456".to_string())
        );
        assert_eq!(installation.launch_command, None);
        assert!(installation.dependencies.is_empty());

        installation.update_checkpoint("checkpoint789");
        assert_eq!(
            installation.nearest_checkpoint,
            Some("checkpoint789".to_string())
        );
        assert_eq!(installation.launch_command, None);
    }

    #[test]
    fn test_retention_policy_defaults() {
        let policy = RetentionPolicyConfig::default();
        let effective = policy.effective();

        assert!(effective.keep_last.is_none());
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

        let rules = effective.to_rules();
        assert_eq!(rules.len(), 4);
        assert!(rules.contains(&("keep-daily".to_string(), "90".to_string())));
        assert!(rules.contains(&("keep-yearly".to_string(), "10".to_string())));
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

        assert_eq!(effective.keep_last, Some(5));
        assert_eq!(effective.keep_daily, 7);
        assert_eq!(effective.keep_weekly, 9);
        assert_eq!(effective.keep_monthly, 11);
        assert_eq!(effective.keep_yearly, 13);

        let rules = effective.to_rules();
        assert_eq!(rules.len(), 5);
        assert!(rules.contains(&("keep-last".to_string(), "5".to_string())));
        assert!(rules.contains(&("keep-yearly".to_string(), "13".to_string())));
    }

    #[test]
    fn test_validate_duplicate_games() {
        let toml_content = r#"
            repo = "~/.test/repo"
            repo_password = "pass"
            
            [[games]]
            name = "Game1"
            
            [[games]]
            name = "Game1"
        "#;

        let config: InstantGameConfig = toml::from_str(toml_content).expect("Parsing failed");
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate game name")
        );
    }

    #[test]
    fn test_validate_duplicate_installations() {
        let toml_content = r#"
            [[installations]]
            game_name = "Game1"
            save_path = "~/.saves/game1"
            
            [[installations]]
            game_name = "Game1"
            save_path = "~/.saves/game1_other"
        "#;

        let config: InstallationsConfig = toml::from_str(toml_content).expect("Parsing failed");
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate installation for game found")
        );
    }
}
