use anyhow::{Context, Result};
use colored::*;
use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::{InstantGameConfig, InstallationsConfig};

/// Display list of all configured games
pub fn list_games() -> Result<()> {
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

/// Display detailed information about a specific game
pub fn show_game_details(game_name: &str) -> Result<()> {
    let config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;
    let installations = InstallationsConfig::load()
        .context("Failed to load installations configuration")?;

    // Find the game in the configuration
    let game = match config.games.iter().find(|g| g.name.0 == game_name) {
        Some(game) => game,
        None => {
            eprintln!("Error: Game '{}' not found in configuration.", game_name.red());
            return Ok(());
        }
    };

    // Find the installation for this game
    let installation = installations.installations.iter()
        .find(|inst| inst.game_name.0 == game_name);

    // Display header
    println!("{}", "Game Information".bold().underline());
    println!();

    // Game name with emoji
    println!("🎮 {}", game.name.0.cyan().bold());
    println!();

    // Description if available
    if let Some(desc) = &game.description {
        println!("📝 {}", desc);
        println!();
    }

    // Configuration section
    println!("{}", "Configuration:".bold());

    if let Some(cmd) = &game.launch_command {
        println!("  🚀 Launch Command: {}", cmd.blue());
    }

    println!("  📁 Save Paths: {}", game.save_paths.len().to_string().green());
    println!();

    // List save paths from configuration
    if !game.save_paths.is_empty() {
        println!("{}", "Configured Save Paths:".bold());
        for save_path in &game.save_paths {
            println!("  • {}: {}", save_path.id.0.cyan(), save_path.description);
        }
        println!();
    }

    // Show actual installation paths if available
    if let Some(install) = installation {
        if !install.saves.is_empty() {
            println!("{}", "Installation Paths:".bold());
            for (path_id, path) in &install.saves {
                let path_display = path.to_tilde_string()
                    .unwrap_or_else(|_| path.as_path().to_string_lossy().to_string());
                println!("  • {}: {}", path_id.0.cyan(), path_display.green());
            }
            println!();
        }
    } else {
        println!("⚠️  No installation data found for this game.");
        println!();
    }

    Ok(())
}