use anyhow::{Context, Result};
use colored::*;
use std::path::PathBuf;

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
    println!("{}", "Initializing game save manager...".bold());

    let mut config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    if config.is_initialized() {
        println!("{}", "Game save manager is already initialized!".yellow());
        println!("Current repository: {}", config.repo.blue());
        return Ok(());
    }

    // Prompt for restic repository using fzf
    let repo = FzfWrapper::input("Enter restic repository path or URL")
        .map_err(|e| anyhow::anyhow!("Failed to get repository input: {}", e))?
        .trim()
        .to_string();

    // Use default if empty
    let repo = if repo.is_empty() {
        let default_path = dirs::data_dir()
            .unwrap_or_else(|| {
                // Expand tilde to home directory
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
                home.join(".local/share")
            })
            .join("instantos")
            .join("games")
            .join("repo");
        default_path.to_string_lossy().to_string()
    } else {
        // Expand tilde in user-provided path if present
        if repo.starts_with("~/") {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
            home.join(&repo[2..]).to_string_lossy().to_string()
        } else {
            repo
        }
    };

    // Use default password as specified in TODO
    let password = "instantgamepassword".to_string();

    // Update config
    config.repo = repo.clone();
    config.repo_password = password.clone();

    // Initialize the repository
    if initialize_restic_repo(&repo, &password, debug)? {
        config.save()?;
        println!("{}", "✓ Game save manager initialized successfully!".green());
        println!("Repository: {}", repo.blue());
    } else {
        return Err(anyhow::anyhow!("Failed to connect to restic repository"));
    }

    Ok(())
}

fn handle_add() -> Result<()> {
    println!("{}", "Add game command not yet implemented".yellow());
    Ok(())
}

fn handle_sync(game_name: Option<String>) -> Result<()> {
    println!("{}", "Sync command not yet implemented".yellow());
    if let Some(name) = game_name {
        println!("Would sync game: {}", name.cyan());
    } else {
        println!("Would sync all games");
    }
    Ok(())
}

fn handle_launch(game_name: String) -> Result<()> {
    println!("{}", "Launch command not yet implemented".yellow());
    println!("Would launch game: {}", game_name.cyan());
    Ok(())
}

fn handle_list() -> Result<()> {
    let config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    if config.games.is_empty() {
        println!("{}", "No games configured yet.".yellow());
        println!("Use 'instant game add' to add a game.");
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


fn initialize_restic_repo(repo: &str, password: &str, debug: bool) -> Result<bool> {
    if debug {
        println!("Initializing restic repository: {}", repo.blue());
    }

    let restic = ResticWrapper::new(repo.to_string(), password.to_string());

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
    if repo.starts_with('/') {
        let path = std::path::Path::new(repo);
        if !path.exists() {
            println!("{}", "Repository path does not exist.".yellow());

            match FzfWrapper::confirm("Would you like to create it?").map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))? {
                ConfirmResult::Yes => {
                    // Create parent directories
                    std::fs::create_dir_all(path).context("Failed to create repository directory")?;
                    println!("{}", "✓ Created repository directory".green());
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("{}", "Repository initialization cancelled.".yellow());
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
