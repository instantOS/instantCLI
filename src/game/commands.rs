use anyhow::{Context, Result};
use colored::*;
use std::path::{Path, PathBuf};
use crate::dot::path_serde::TildePath;

use super::cli::GameCommands;
use super::config::*;
use crate::fzf_wrapper::FzfWrapper;
use crate::fzf_wrapper::ConfirmResult;
use crate::restic::ResticWrapper;

pub fn handle_game_command(command: GameCommands, debug: bool) -> Result<()> {
    match command {
        GameCommands::Init => handle_init(debug),
        GameCommands::Add => handle_add(),
        GameCommands::Sync { game_name } => handle_sync(game_name),
        GameCommands::Launch { game_name } => handle_launch(game_name),
        GameCommands::List => handle_list(),
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

    // For now, use the default save path structure
    // In the future, this could be expanded to prompt for multiple save paths

    // Add the game to the configuration
    config.games.push(game);
    config.save()?;

    // Create the installation entry
    let installation = GameInstallation::new(game_name.clone());
    installations.installations.push(installation);
    installations.save()?;

    FzfWrapper::message(&format!(
        "✓ Game '{}' added successfully!\n\nYou can now configure save paths manually or use 'instant game edit' when implemented.",
        game_name
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
