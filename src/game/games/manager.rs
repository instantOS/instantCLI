use super::{selection::select_game_interactive, validation::*};
use crate::dot::path_serde::TildePath;
use crate::fzf_wrapper::{ConfirmResult, FzfWrapper};
use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
use anyhow::{Context, Result};

/// Manage game CRUD operations
pub struct GameManager;

impl GameManager {
    /// Add a new game to the configuration
    pub fn add_game() -> Result<()> {
        let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;

        let mut installations =
            InstallationsConfig::load().context("Failed to load installations configuration")?;

        // Check if game manager is initialized
        if !validate_game_manager_initialized()? {
            return Ok(());
        }

        // Get and validate game name
        let game_name = Self::get_game_name(&config)?;

        // Get optional description
        let description = Self::get_game_description()?;

        // Get optional launch command
        let launch_command = Self::get_launch_command()?;

        // Create the game configuration
        let mut game = Game::new(game_name.clone());

        if !description.is_empty() {
            game.description = Some(description);
        }

        if !launch_command.is_empty() {
            game.launch_command = Some(launch_command);
        }

        // Get save path
        let save_path = Self::get_save_path()?;

        // Add the game to the configuration
        config.games.push(game);
        config.save()?;

        // Create the installation entry with the save path
        let installation = GameInstallation::new(game_name.clone(), save_path.clone());
        installations.installations.push(installation);
        installations.save()?;

        println!("✓ Game '{game_name}' added successfully!");
        println!("Game configuration saved with save path: {save_path:?}");

        Ok(())
    }

    /// Remove a game from the configuration
    pub fn remove_game(game_name: Option<String>) -> Result<()> {
        let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;

        let mut installations =
            InstallationsConfig::load().context("Failed to load installations configuration")?;

        let game_name = match game_name {
            Some(name) => name,
            None => match select_game_interactive(Some("Select a game to remove:"))? {
                Some(name) => name,
                None => return Ok(()),
            },
        };

        // Find the game in the configuration
        let game_index = config.games.iter().position(|g| g.name.0 == game_name);

        if game_index.is_none() {
            eprintln!("Game '{game_name}' not found in configuration.");
            return Ok(());
        }

        let game_index = game_index.unwrap();
        let game = &config.games[game_index];

        // Show game details and ask for confirmation with improved formatting
        match FzfWrapper::builder()
            .confirm(format!(
                "Are you sure you want to remove the following game?\n\n\
                 Game: {}\n\
                 Description: {}\n\
                 Launch command: {}\n\n\
                 This will remove the game from your configuration and save path mapping.",
                game.name.0,
                game.description.as_deref().unwrap_or("None"),
                game.launch_command.as_deref().unwrap_or("None")
            ))
            .yes_text("Remove Game")
            .no_text("Keep Game")
            .show()
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
        {
            ConfirmResult::Yes => {
                // Remove the game from the configuration
                config.games.remove(game_index);
                config.save()?;

                // Remove any installations for this game
                installations
                    .installations
                    .retain(|inst| inst.game_name.0 != game_name);
                installations.save()?;

                println!("✓ Game '{game_name}' removed successfully!");
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                println!("Game removal cancelled.");
            }
        }

        Ok(())
    }

    /// Get and validate game name from user input
    fn get_game_name(config: &InstantGameConfig) -> Result<String> {
        let game_name = FzfWrapper::input("Enter game name")
            .map_err(|e| anyhow::anyhow!("Failed to get game name input: {}", e))?
            .trim()
            .to_string();

        if !validate_non_empty(&game_name, "Game name")? {
            return Err(anyhow::anyhow!("Game name cannot be empty"));
        }

        // Check if game already exists
        if config.games.iter().any(|g| g.name.0 == game_name) {
            eprintln!("Game '{game_name}' already exists!");
            return Err(anyhow::anyhow!("Game already exists"));
        }

        Ok(game_name)
    }

    /// Get optional game description from user input
    fn get_game_description() -> Result<String> {
        Ok(FzfWrapper::input("Enter game description (optional)")
            .map_err(|e| anyhow::anyhow!("Failed to get description input: {}", e))?
            .trim()
            .to_string())
    }

    /// Get optional launch command from user input
    fn get_launch_command() -> Result<String> {
        Ok(FzfWrapper::input("Enter launch command (optional)")
            .map_err(|e| anyhow::anyhow!("Failed to get launch command input: {}", e))?
            .trim()
            .to_string())
    }

    /// Get save path from user input with validation
    fn get_save_path() -> Result<TildePath> {
        let save_path_input = FzfWrapper::input(
            "Enter path where save files are located (e.g., ~/.local/share/game-name/saves)",
        )
        .map_err(|e| anyhow::anyhow!("Failed to get save path input: {}", e))?
        .trim()
        .to_string();

        if !validate_non_empty(&save_path_input, "Save path")? {
            return Err(anyhow::anyhow!("Save path cannot be empty"));
        }

        // Convert the input path to a TildePath
        let save_path = TildePath::from_str(&save_path_input)
            .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?;

        // Check if the save path exists
        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "Save path '{save_path_input}' does not exist. Would you like to create it?"
            ))
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => {
                    std::fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    println!("✓ Created save directory: {save_path_input}");
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Game addition cancelled: save path does not exist.");
                    return Err(anyhow::anyhow!("Save path does not exist"));
                }
            }
        }

        Ok(save_path)
    }
}
