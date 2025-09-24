use anyhow::{Context, Result};
use colored::*;
use std::path::{Path, PathBuf};
use crate::dot::path_serde::TildePath;

use super::cli::GameCommands;
use super::config::*;
use crate::fzf_wrapper::FzfWrapper;
use crate::fzf_wrapper::ConfirmResult;
use crate::fzf_wrapper::FzfSelectable;
use crate::menu::protocol::FzfPreview;
use crate::restic::ResticWrapper;

impl FzfSelectable for Game {
    fn fzf_display_text(&self) -> String {
        self.name.0.clone()
    }

    fn fzf_key(&self) -> String {
        self.name.0.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        match &self.description {
            Some(desc) => FzfPreview::Text(desc.clone()),
            None => FzfPreview::None,
        }
    }
}

pub fn handle_game_command(command: GameCommands, debug: bool) -> Result<()> {
    match command {
        GameCommands::Init => handle_init(debug),
        GameCommands::Add => handle_add(),
        GameCommands::Sync { game_name } => handle_sync(game_name),
        GameCommands::Launch { game_name } => handle_launch(game_name),
        GameCommands::List => handle_list(),
        GameCommands::Remove { game_name } => handle_remove(game_name),
    }
}

fn handle_init(debug: bool) -> Result<()> {
    FzfWrapper::message("Initializing game save manager...").context("Failed to show initialization message")?;

    let mut config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    if config.is_initialized() {
        FzfWrapper::message(&format!(
            "Game save manager is already initialized!\n\nCurrent repository: {}",
            config.repo.to_tilde_string()
                .unwrap_or_else(|_| config.repo.as_path().to_string_lossy().to_string())
        )).context("Failed to show already initialized message")?;
        return Ok(());
    }

    // Prompt for restic repository using fzf
    //TODO: see if the error handling can be improved
    let repo = FzfWrapper::input("Enter restic repository path or URL")
        .map_err(|e| anyhow::anyhow!("Failed to get repository input: {}", e))?
        .trim()
        .to_string();

    // Use default if empty
    let repo = if repo.is_empty() {
        let default_path = dirs::data_dir()
            //TODO: unwrap is fine
            .unwrap_or_else(|| {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
                home.join(".local/share")
            })
            .join("instantos")
            .join("games")
            .join("repo");
        TildePath::new(default_path)
    } else {
        // Use TildePath to handle tilde expansion automatically
        TildePath::from_str(&repo)?
    };

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
        )).context("Failed to show success message")?;
    } else {
        return Err(anyhow::anyhow!("Failed to connect to restic repository"));
    }

    Ok(())
}

fn handle_add() -> Result<()> {
    let mut config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    let mut installations = InstallationsConfig::load()
        .context("Failed to load installations configuration")?;

    // Check if game manager is initialized
    if !config.is_initialized() {
        FzfWrapper::message(
            "Game save manager is not initialized!\n\nPlease run 'instant game init' first."
        ).context("Failed to show initialization required message")?;
        return Ok(());
    }

    // Prompt for game name
    let game_name = FzfWrapper::input("Enter game name")
        .map_err(|e| anyhow::anyhow!("Failed to get game name input: {}", e))?
        .trim()
        .to_string();

    if game_name.is_empty() {
        FzfWrapper::message("Game name cannot be empty.").context("Failed to show validation error")?;
        return Ok(());
    }

    // Check if game already exists
    if config.games.iter().any(|g| g.name.0 == game_name) {
        FzfWrapper::message(&format!(
            "Game '{}' already exists!",
            game_name
        )).context("Failed to show duplicate game error")?;
        return Ok(());
    }

    // Prompt for optional description
    let description = FzfWrapper::input("Enter game description (optional)")
        .map_err(|e| anyhow::anyhow!("Failed to get description input: {}", e))?
        .trim()
        .to_string();

    // Prompt for optional launch command
    let launch_command = FzfWrapper::input("Enter launch command (optional)")
        .map_err(|e| anyhow::anyhow!("Failed to get launch command input: {}", e))?
        .trim()
        .to_string();

    // Create the game configuration
    let mut game = Game::new(game_name.clone());

    if !description.is_empty() {
        game.description = Some(description);
    }

    if !launch_command.is_empty() {
        game.launch_command = Some(launch_command);
    }

    // Prompt for save path location
    let save_path_input = FzfWrapper::input("Enter path where save files are located (e.g., ~/.local/share/game-name/saves)")
        .map_err(|e| anyhow::anyhow!("Failed to get save path input: {}", e))?
        .trim()
        .to_string();

    if save_path_input.is_empty() {
        FzfWrapper::message("Save path cannot be empty.").context("Failed to show validation error")?;
        return Ok(());
    }

    // Convert the input path to a TildePath
    let save_path = TildePath::from_str(&save_path_input)
        .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?;

    // Check if the save path exists
    if !save_path.as_path().exists() {
        match FzfWrapper::confirm(&format!(
            "Save path '{}' does not exist. Would you like to create it?",
            save_path_input
        )).map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))? {
            ConfirmResult::Yes => {
                std::fs::create_dir_all(save_path.as_path())
                    .context("Failed to create save directory")?;
                FzfWrapper::message(&format!("✓ Created save directory: {}", save_path_input))
                    .context("Failed to show directory created message")?;
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                FzfWrapper::message("Game addition cancelled: save path does not exist.")
                    .context("Failed to show cancellation message")?;
                return Ok(());
            }
        }
    }

    // Add the game to the configuration
    config.games.push(game);
    config.save()?;

    // Create the installation entry with the save path
    let installation = GameInstallation::new(game_name.clone())
        .add_save_path("saves", save_path);
    installations.installations.push(installation);
    installations.save()?;

    FzfWrapper::message(&format!(
        "✓ Game '{}' added successfully!\n\nGame configuration saved with save path: {}",
        game_name, save_path_input
    )).context("Failed to show success message")?;

    Ok(())
}

fn handle_sync(game_name: Option<String>) -> Result<()> {
    FzfWrapper::message("Sync command not yet implemented").context("Failed to show not implemented message")?;
    if let Some(name) = game_name {
        println!("Would sync game: {}", name.cyan());
    } else {
        println!("Would sync all games");
    }
    Ok(())
}

fn handle_launch(game_name: String) -> Result<()> {
    FzfWrapper::message(&format!(
        "Launch command not yet implemented\n\nWould launch game: {}",
        game_name
    )).context("Failed to show not implemented message")?;
    Ok(())
}

fn handle_remove(game_name: Option<String>) -> Result<()> {
    let mut config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    let mut installations = InstallationsConfig::load()
        .context("Failed to load installations configuration")?;

    if config.games.is_empty() {
        FzfWrapper::message(
            "No games configured yet.\n\nUse 'instant game add' to add a game."
        ).context("Failed to show empty games message")?;
        return Ok(());
    }

    // Determine which game to remove
    let game_name = match game_name {
        Some(name) => name,
        None => {
            // Show FZF menu to select game
            FzfWrapper::message("Select a game to remove:").context("Failed to show selection prompt")?;
            let selected = FzfWrapper::select_one(config.games.clone())
                .map_err(|e| anyhow::anyhow!("Failed to select game: {}", e))?;

            match selected {
                Some(game) => game.name.0,
                None => {
                    FzfWrapper::message("No game selected.")
                        .context("Failed to show no selection message")?;
                    return Ok(());
                }
            }
        }
    };

    // Find the game in the configuration
    let game_index = config.games.iter().position(|g| g.name.0 == game_name);

    if game_index.is_none() {
        FzfWrapper::message(&format!(
            "Game '{}' not found in configuration.",
            game_name
        )).context("Failed to show game not found message")?;
        return Ok(());
    }

    let game_index = game_index.unwrap();
    let game = &config.games[game_index];

    // Show game details and ask for confirmation with improved formatting
    match FzfWrapper::confirm_builder()
        .message(format!(
            "Are you sure you want to remove the following game?\n\n\
             Game: {}\n\
             Description: {}\n\
             Launch command: {}\n\
             Save paths: {}\n\n\
             This will remove the game from your configuration and all save path mappings.",
            game.name.0,
            game.description.as_deref().unwrap_or("None"),
            game.launch_command.as_deref().unwrap_or("None"),
            game.save_paths.len()
        ))
        .yes_text("Remove Game")
        .no_text("Keep Game")
        .show()
        .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))? {
        ConfirmResult::Yes => {
            // Remove the game from the configuration
            config.games.remove(game_index);
            config.save()?;

            // Remove any installations for this game
            installations.installations.retain(|inst| inst.game_name.0 != game_name);
            installations.save()?;

            FzfWrapper::message_builder()
                .message(format!(
                    "✓ Game '{}' removed successfully!",
                    game_name
                ))
                .title("Success")
                .show()
                .context("Failed to show success message")?;
        }
        ConfirmResult::No | ConfirmResult::Cancelled => {
            FzfWrapper::message_builder()
                .message("Game removal cancelled.")
                .title("Cancelled")
                .show()
                .context("Failed to show cancellation message")?;
        }
    }

    Ok(())
}

fn handle_list() -> Result<()> {
    let config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    if config.games.is_empty() {
        FzfWrapper::message(
            "No games configured yet.\n\nUse 'instant game add' to add a game."
        ).context("Failed to show empty games message")?;
        return Ok(());
    }

    // Display header
    println!("{}", "Configured Games".bold().underline());
    println!();

    for game in &config.games {
        // Game name with status indicator
        println!("  {} {}", "🎮".bright_blue(), game.name.0.cyan().bold());

        if let Some(desc) = &game.description {
            println!("    Description: {}", desc);
        }

        println!("    Save paths: {}", game.save_paths.len().to_string().green());

        if let Some(cmd) = &game.launch_command {
            println!("    Launch command: {}", cmd.blue());
        }

        // List save paths
        if !game.save_paths.is_empty() {
            println!("    Save paths:");
            for save_path in &game.save_paths {
                println!("      • {}: {}", save_path.id.0.cyan(), save_path.description);
            }
        }

        println!();
    }

    // Summary
    println!(
        "Total: {} game{} configured",
        config.games.len().to_string().bold(),
        if config.games.len() == 1 { "" } else { "s" }
    );

    Ok(())
}


fn initialize_restic_repo(repo: &Path, password: &str, debug: bool) -> Result<bool> {
    if debug {
        println!("Initializing restic repository: {}", repo.to_string_lossy().blue());
    }

    let restic = ResticWrapper::new(repo.to_string_lossy().to_string(), password.to_string());

    // Check if repository already exists
    match restic.repository_exists() {
        Ok(exists) => {
            if exists {
                if debug {
                    println!("{}", "✓ Repository already exists and is accessible".green());
                }
                return Ok(true);
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to check repository existence: {}", e));
        }
    }

    // Repository doesn't exist, initialize it
    if repo.is_absolute() {
        if !repo.exists() {
            FzfWrapper::message("Repository path does not exist.").context("Failed to show path error message")?;

            match FzfWrapper::confirm("Would you like to create it?").map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))? {
                ConfirmResult::Yes => {
                    // Create parent directories
                    std::fs::create_dir_all(repo).context("Failed to create repository directory")?;
                    FzfWrapper::message("✓ Created repository directory").context("Failed to show directory created message")?;
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    FzfWrapper::message("Repository initialization cancelled.").context("Failed to show cancellation message")?;
                    return Ok(false);
                }
            }
        }
    }

    // Initialize the repository
    if debug {
        println!("{}", "Creating new restic repository...".blue());
    }

    match restic.init_repository() {
        Ok(()) => {
            if debug {
                println!("{}", "✓ Repository initialized successfully".green());
            }
            Ok(true)
        }
        Err(e) => {
            Err(anyhow::anyhow!("Failed to initialize repository: {}", e))
        }
    }
}
