use anyhow::{Context, Result};
use std::path::PathBuf;

/// Centralized path management for instantCLI
/// This module provides a single source of truth for all application paths

/// Get the main instant config directory
pub fn instant_config_dir() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Unable to determine user config directory")?
        .join("instant");

    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating config directory at {}", config_dir.display()))?;

    Ok(config_dir)
}

/// Get the main instant data directory
pub fn instant_data_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("instant");

    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("creating data directory at {}", data_dir.display()))?;

    Ok(data_dir)
}

/// Get the instant dots configuration file path
pub fn dots_config_file() -> Result<PathBuf> {
    Ok(instant_config_dir()?.join("dots.toml"))
}

/// Get the instant settings configuration file path
pub fn settings_config_file() -> Result<PathBuf> {
    Ok(instant_config_dir()?.join("settings.toml"))
}

/// Get the instant users configuration file path
pub fn users_config_file() -> Result<PathBuf> {
    Ok(instant_config_dir()?.join("users.toml"))
}

/// Get the instant games config directory
pub fn games_config_dir() -> Result<PathBuf> {
    let games_dir = instant_config_dir()?.join("games");
    std::fs::create_dir_all(&games_dir)
        .with_context(|| format!("creating games config directory at {}", games_dir.display()))?;
    Ok(games_dir)
}

/// Get the instant games configuration file path
pub fn games_config_file() -> Result<PathBuf> {
    Ok(games_config_dir()?.join("games.toml"))
}

/// Get the instant dots repository directory
pub fn dots_repo_dir() -> Result<PathBuf> {
    let dots_dir = instant_data_dir()?.join("dots");
    std::fs::create_dir_all(&dots_dir)
        .with_context(|| format!("creating dots repo directory at {}", dots_dir.display()))?;
    Ok(dots_dir)
}

/// Get the instant database directory and file
pub fn instant_database_file() -> Result<PathBuf> {
    Ok(instant_data_dir()?.join("instant.db"))
}

/// Get the instant video data directory
pub fn instant_video_dir() -> Result<PathBuf> {
    let video_dir = instant_data_dir()?.join("video");
    std::fs::create_dir_all(&video_dir)
        .with_context(|| format!("creating video directory at {}", video_dir.display()))?;
    Ok(video_dir)
}

/// Get the instant games repository directory (for restic)
pub fn instant_games_repo_dir() -> Result<PathBuf> {
    let games_repo_dir = instant_data_dir()?.join("games").join("repo");
    std::fs::create_dir_all(&games_repo_dir)
        .with_context(|| format!("creating games repo directory at {}", games_repo_dir.display()))?;
    Ok(games_repo_dir)
}

/// Get the instant restic logs directory
pub fn instant_restic_logs_dir() -> Result<PathBuf> {
    let logs_dir = instant_data_dir()?.join("restic_logs");
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("creating restic logs directory at {}", logs_dir.display()))?;
    Ok(logs_dir)
}

/// Get the instant video configuration file path
pub fn video_config_file() -> Result<PathBuf> {
    Ok(instant_config_dir()?.join("video.toml"))
}

/// Get the default local repository path for games (fallback)
pub fn default_games_repo_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
            home.join(".local/share")
        })
        .join("instant")
        .join("games")
        .join("repo")
}