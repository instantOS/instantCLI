use anyhow::{Context, Result};
use colored::*;
use std::io::{self, Write};

use super::cli::GameCommands;
use super::config::*;

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
    println!("Initializing game save manager...");

    let mut config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    if config.is_initialized() {
        println!("{}", "Game save manager is already initialized!".yellow());
        println!("Current repository: {}", config.repo);
        return Ok(());
    }

    // Prompt for restic repository
    let repo = prompt_for_input("Enter restic repository path or URL:", Some("/path/to/restic/repo"))?;

    // Prompt for optional password
    let password = prompt_for_password("Enter restic repository password (leave empty for no password):");

    // Update config
    config.repo = repo.clone();
    config.repo_password = password;

    // Test the repository
    if test_restic_repo(&repo, config.repo_password.as_deref(), debug)? {
        config.save()?;
        println!("{}", "✓ Game save manager initialized successfully!".green());
        println!("Repository: {}", repo);
    } else {
        return Err(anyhow::anyhow!("Failed to connect to restic repository"));
    }

    Ok(())
}

fn handle_add() -> Result<()> {
    println!("Add game command not yet implemented");
    Ok(())
}

fn handle_sync(game_name: Option<String>) -> Result<()> {
    println!("Sync command not yet implemented");
    if let Some(name) = game_name {
        println!("Would sync game: {}", name);
    } else {
        println!("Would sync all games");
    }
    Ok(())
}

fn handle_launch(game_name: String) -> Result<()> {
    println!("Launch command not yet implemented");
    println!("Would launch game: {}", game_name);
    Ok(())
}

fn handle_list() -> Result<()> {
    let config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    if config.games.is_empty() {
        println!("No games configured yet. Use 'instant game add' to add a game.");
        return Ok(());
    }

    println!("Configured games:");
    for game in &config.games {
        println!("  - {}", game.name);
        if let Some(desc) = &game.description {
            println!("    Description: {}", desc);
        }
        println!("    Save paths: {}", game.save_paths.len());
        if let Some(cmd) = &game.launch_command {
            println!("    Launch command: {}", cmd);
        }
        println!();
    }

    Ok(())
}

fn prompt_for_input(prompt: &str, default: Option<&str>) -> Result<String> {
    print!("{}", prompt);
    if let Some(default) = default {
        print!(" [{}]", default);
    }
    print!(": ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(input.to_string())
    }
}

fn prompt_for_password(prompt: &str) -> Option<String> {
    print!("{}", prompt);
    io::stdout().flush().ok();

    // Simple password input (not hidden for now)
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok()?;

    let input = input.trim();
    if input.is_empty() {
        None
    } else {
        Some(input.to_string())
    }
}

fn test_restic_repo(repo: &str, _password: Option<&str>, debug: bool) -> Result<bool> {
    if debug {
        println!("Testing restic repository: {}", repo);
    }

    // For now, just check if the path exists for local repos
    // In a full implementation, we would use rustic_core to actually test the repository
    if repo.starts_with('/') {
        let path = std::path::Path::new(repo);
        if path.exists() {
            return Ok(true);
        } else {
            println!("Repository path does not exist. Would you like to create it? (y/N)");
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim().to_lowercase() == "y" {
                std::fs::create_dir_all(path).context("Failed to create repository directory")?;
                println!("Created repository directory");
                return Ok(true);
            }
        }
    } else {
        // For remote repos, assume they're valid for now
        return Ok(true);
    }

    Ok(false)
}