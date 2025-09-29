use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::utils::save_files::{
    format_file_size, format_system_time_for_display, get_save_directory_info,
};
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::*;
use serde_json::json;

/// Display list of all configured games
pub fn list_games() -> Result<()> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    // Prepare data array
    let games_data: Vec<serde_json::Value> = config
        .games
        .iter()
        .map(|g| json!({
            "name": g.name.0,
            "description": g.description,
            "launch_command": g.launch_command
        }))
        .collect();

    // Build human-friendly text block
    let mut text = String::new();
    if config.games.is_empty() {
        text.push_str("No games configured yet.\n");
        text.push_str(&format!("Use '{} game add' to add a game.\n", env!("CARGO_BIN_NAME")));
    } else {
        text.push_str(&format!("{}\n\n", "Configured Games".bold().underline()));
        for game in &config.games {
            text.push_str(&format!("  {} {}\n", Icons::INFO.bright_blue(), game.name.0.cyan().bold()));
            if let Some(desc) = &game.description {
                text.push_str(&format!("    Description: {}\n", desc));
            }
            if let Some(cmd) = &game.launch_command {
                text.push_str(&format!("    Launch command: {}\n", cmd.blue()));
            }
            text.push('\n');
        }
        text.push_str(&format!(
            "Total: {} game{} configured",
            config.games.len().to_string().bold(),
            if config.games.len() == 1 { "" } else { "s" }
        ));
    }

    emit(
        Level::Info,
        "game.list",
        &text,
        Some(json!({
            "count": config.games.len(),
            "games": games_data
        })),
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
            error(
                "game.show.not_found",
                &format!("Game '{}' not found in configuration.", game_name.red()),
            );
            return Ok(());
        }
    };

    // Find the installation for this game
    let installation = installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name);

    // Build structured data and a pleasant text summary, then emit via UI
    let mut launch_command = None::<String>;
    if let Some(cmd) = &game.launch_command {
        launch_command = Some(cmd.clone());
    }

    let mut install_data = None::<serde_json::Value>;
    let mut install_text = String::new();
    if let Some(install) = installation {
        let path_display = install
            .save_path
            .to_tilde_string()
            .unwrap_or_else(|_| install.save_path.as_path().to_string_lossy().to_string());

        // Try to read local saves info
        let save_info_result = get_save_directory_info(install.save_path.as_path());
        match save_info_result {
            Ok(save_info) => {
                install_data = Some(json!({
                    "save_path": path_display,
                    "local_saves": {
                        "last_modified": format_system_time_for_display(save_info.last_modified),
                        "file_count": save_info.file_count,
                        "total_size": format_file_size(save_info.total_size)
                    }
                }));

                if save_info.file_count > 0 {
                    install_text.push_str(&format!(
                        "Installation:\n  {} Save Path: {}\n   Local Saves:\n     • Last modified: {}\n     • Files: {}\n     • Total size: {}\n",
                        Icons::FOLDER,
                        path_display.green(),
                        format_system_time_for_display(save_info.last_modified),
                        save_info.file_count,
                        format_file_size(save_info.total_size)
                    ));
                } else {
                    install_text.push_str(&format!(
                        "Installation:\n  {} Save Path: {}\n   Local Saves: No save files found\n",
                        Icons::FOLDER,
                        path_display.green()
                    ));
                }
            }
            Err(e) => {
                install_data = Some(json!({
                    "save_path": path_display,
                    "local_saves_error": e.to_string()
                }));
                install_text.push_str(&format!(
                    "Installation:\n  {} Save Path: {}\n   Local Saves: Unable to analyze save directory ({})\n",
                    Icons::FOLDER,
                    path_display.green(),
                    e.to_string().to_lowercase()
                ));
            }
        }
    } else {
        install_text.push_str(&format!(
            "{}  No installation data found for this game.\n",
            Icons::WARN
        ));
    }

    // Build the top text block
    let mut text_block = String::new();
    text_block.push_str(&format!("{}\n\n", "Game Information".bold().underline()));
    text_block.push_str(&format!(
        "{} {}\n\n",
        Icons::INFO,
        game.name.0.cyan().bold()
    ));
    if let Some(desc) = &game.description {
        text_block.push_str(&format!(" {}\n\n", desc));
    }
    text_block.push_str(&format!("{}\n", "Configuration:".bold()));
    if let Some(cmd) = &launch_command {
        text_block.push_str(&format!("   Launch Command: {}\n\n", cmd.blue()));
    } else {
        text_block.push_str("\n");
    }
    text_block.push_str(&install_text);

    // Emit combined event (text+data)
    emit(
        Level::Info,
        "game.show.details",
        &text_block,
        Some(json!({
            "game": {
                "name": game.name.0,
                "description": game.description,
                "launch_command": launch_command
            },
            "installation": install_data
        })),
    );

    Ok(())
}
