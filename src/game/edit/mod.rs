mod editors;
mod menu;
mod state;

use anyhow::{Context, Result, anyhow};

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::selection::select_game_interactive;

use state::EditState;

/// Main entry point for editing a game
pub fn edit_game(game_name: Option<String>) -> Result<()> {
    // Select game if not provided
    let game_name = match game_name {
        Some(name) => name,
        None => match select_game_interactive(None)? {
            Some(name) => name,
            None => return Ok(()),
        },
    };

    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations = InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Find the game and installation indices
    let game_index = game_config
        .games
        .iter()
        .position(|g| g.name.0 == game_name)
        .ok_or_else(|| anyhow!("Game '{}' not found in games.toml", game_name))?;

    let installation_index = installations
        .installations
        .iter()
        .position(|i| i.game_name.0 == game_name);

    // Create edit state and run the menu
    let mut state = EditState::new(game_config, installations, game_index, installation_index);
    menu::run_edit_menu(&game_name, &mut state)
}

