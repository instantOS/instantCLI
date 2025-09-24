use anyhow::{Context, Result};
use colored::*;
use std::io::{self, Write};
use std::path::PathBuf;

use super::cli::GameCommands;
use super::config::*;
use crate::fzf_wrapper::FzfWrapper;

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
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("instantos")
            .join("games")
            .join("repo");
        default_path.to_string_lossy().to_string()
    } else {
        repo
    };

    // Use default password as specified in TODO
    let password = Some("instantgamepassword".to_string());

    // Update config
    config.repo = repo.clone();
    config.repo_password = password;

    // Test the repository
    if test_restic_repo(&repo, config.repo_password.as_deref(), debug)? {
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

//TODO: this function should be gone entirely, just use fzf wrapper input in places where is currently
//used
fn prompt_for_input(prompt: &str, default: Option<&str>) -> Result<String> {
    if let Some(default) = default {
        print!("{} [{}]: ", prompt, default);
    } else {
        print!("{}: ", prompt);
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("reading input from stdin")?;

    let input = input.trim();
    if input.is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(input.to_string())
    }
}

//TODO: for now, do not prompt for a password at all, just default to `instantgamepassword`
fn prompt_for_password(prompt: &str) -> Option<String> {
    print!("{}: ", prompt);
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
        println!("Testing restic repository: {}", repo.blue());
    }

    // For now, just check if the path exists for local repos
    // In a full implementation, we would use rustic_core to actually test the repository
    if repo.starts_with('/') {
        let path = std::path::Path::new(repo);
        if path.exists() {
            if debug {
                println!("{}", "✓ Repository path exists".green());
            }
            return Ok(true);
        } else {
            println!("{}", "Repository path does not exist.".yellow());
            print!("Would you like to create it? (y/N): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .context("reading user response")?;

            if input.trim().to_lowercase() == "y" {
                std::fs::create_dir_all(path).context("Failed to create repository directory")?;
                println!("{}", "✓ Created repository directory".green());
                return Ok(true);
            } else {
                println!("{}", "Repository initialization cancelled.".yellow());
            }
        }
    } else {
        // For remote repos, assume they're valid for now
        if debug {
            println!("{}", "✓ Remote repository URL accepted".green());
        }
        return Ok(true);
    }

    Ok(false)
}
