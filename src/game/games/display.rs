use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::utils::save_files::{get_save_directory_info, format_system_time_for_display, format_file_size};
use anyhow::{Context, Result};
use colored::*;

/// Display list of all configured games
pub fn list_games() -> Result<()> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    if config.games.is_empty() {
        FzfWrapper::message("No games configured yet.\n\nUse 'instant game add' to add a game.")
            .context("Failed to show empty games message")?;
        return Ok(());
    }

    // Display header
    println!("{}", "Configured Games".bold().underline());
    println!();

    for game in &config.games {
        // Game name with status indicator
        println!("  {} {}", "🎮".bright_blue(), game.name.0.cyan().bold());

        if let Some(desc) = &game.description {
            println!("    Description: {desc}");
        }

        if let Some(cmd) = &game.launch_command {
            println!("    Launch command: {}", cmd.blue());
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
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Find the game in the configuration
    let game = match config.games.iter().find(|g| g.name.0 == game_name) {
        Some(game) => game,
        None => {
            eprintln!(
                "Error: Game '{}' not found in configuration.",
                game_name.red()
            );
            return Ok(());
        }
    };

    // Find the installation for this game
    let installation = installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name);

    // Display header
    println!("{}", "Game Information".bold().underline());
    println!();

    // Game name with emoji
    println!("🎮 {}", game.name.0.cyan().bold());
    println!();

    // Description if available
    if let Some(desc) = &game.description {
        println!("📝 {desc}");
        println!();
    }

    // Configuration section
    println!("{}", "Configuration:".bold());

    if let Some(cmd) = &game.launch_command {
        println!("  🚀 Launch Command: {}", cmd.blue());
    }
    println!();

    // Show actual installation path if available
    if let Some(install) = installation {
        println!("{}", "Installation:".bold());
        let path_display = install
            .save_path
            .to_tilde_string()
            .unwrap_or_else(|_| install.save_path.as_path().to_string_lossy().to_string());
        println!("  📁 Save Path: {}", path_display.green());

        // Get save directory information
        match get_save_directory_info(install.save_path.as_path()) {
            Ok(save_info) => {
                if save_info.file_count > 0 {
                    println!("  💾 Local Saves:");
                    println!("     • Last modified: {}", format_system_time_for_display(save_info.last_modified));
                    println!("     • Files: {}", save_info.file_count);
                    println!("     • Total size: {}", format_file_size(save_info.total_size));
                } else {
                    println!("  💾 Local Saves: No save files found");
                }
            }
            Err(e) => {
                println!("  💾 Local Saves: Unable to analyze save directory ({})", e.to_string().to_lowercase());
            }
        }
        println!();
    } else {
        println!("⚠️  No installation data found for this game.");
        println!();
    }

    Ok(())
}
