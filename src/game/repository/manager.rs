use super::init::initialize_restic_repo;
use crate::dot::path_serde::TildePath;
use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::InstantGameConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Manage restic repository initialization and configuration
pub struct RepositoryManager;

impl RepositoryManager {
    /// Initialize the game save backup system
    pub fn initialize_game_manager(debug: bool) -> Result<()> {
        FzfWrapper::message("Initializing game save manager...")
            .context("Failed to show initialization message")?;

        let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;

        if config.is_initialized() {
            FzfWrapper::message(&format!(
                "Game save manager is already initialized!\n\nCurrent repository: {}",
                config.repo.to_tilde_string().unwrap_or_else(|_| config
                    .repo
                    .as_path()
                    .to_string_lossy()
                    .to_string())
            ))
            .context("Failed to show already initialized message")?;
            return Ok(());
        }

        // Prompt for restic repository using fzf
        let repo = Self::get_repository_path()?;
        let password = "instantgamepassword".to_string();

        // Update config
        config.repo = repo.clone();
        config.repo_password = password.clone();

        // Initialize the repository
        if initialize_restic_repo(repo.as_path(), &password, debug)? {
            config.save()?;
            FzfWrapper::message(&format!(
                "✓ Game save manager initialized successfully!\n\nRepository: {}",
                repo.to_tilde_string()
                    .unwrap_or_else(|_| repo.as_path().to_string_lossy().to_string())
            ))
            .context("Failed to show success message")?;
        } else {
            return Err(anyhow::anyhow!("Failed to connect to restic repository"));
        }

        Ok(())
    }

    /// Get repository path from user input or use default
    fn get_repository_path() -> Result<TildePath> {
        // Prompt for restic repository using fzf
        let repo_input = FzfWrapper::input("Enter restic repository path or URL")
            .map_err(|e| anyhow::anyhow!("Failed to get repository input: {}", e))?
            .trim()
            .to_string();

        // Use default if empty
        if repo_input.is_empty() {
            let default_path = dirs::data_dir()
                .unwrap_or_else(|| {
                    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
                    home.join(".local/share")
                })
                .join("instantos")
                .join("games")
                .join("repo");
            Ok(TildePath::new(default_path))
        } else {
            // Use TildePath to handle tilde expansion automatically
            TildePath::from_str(&repo_input)
        }
    }
}
