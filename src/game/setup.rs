use crate::ui::prelude::*;
use anyhow::{Context, Result};
use std::collections::HashSet;

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::manager::{AddGameOptions, GameManager};
use crate::game::games::validation::validate_game_manager_initialized;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

mod install;
mod paths;
mod restic;

/// Set up games that have been added but don't have installations configured on this device
pub fn setup_uninstalled_games() -> Result<()> {
    let mut game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    if !validate_game_manager_initialized()? {
        return Ok(());
    }

    restic::maybe_setup_restic_games(&mut game_config, &mut installations)?;

    let uninstalled_games = find_uninstalled_games(&game_config, &installations)?;

    if uninstalled_games.is_empty() {
        GameManager::add_game(AddGameOptions::default())?;
        return Ok(());
    }

    println!(
        "Found {} game(s) that need to be set up on this device:\n",
        uninstalled_games.len()
    );

    for game_name in &uninstalled_games {
        println!("  • {game_name}");
    }

    println!();

    for game_name in uninstalled_games {
        if let Err(error) = install::setup_single_game(&game_name, &game_config, &mut installations)
        {
            emit(
                Level::Error,
                "game.setup.failed",
                &format!(
                    "{} Failed to set up game '{game_name}': {error}",
                    char::from(NerdFont::CrossCircle)
                ),
                None,
            );

            match FzfWrapper::confirm("Would you like to continue setting up the remaining games?")
                .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {e}"))?
            {
                ConfirmResult::Yes => continue,
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Setup cancelled by user.");
                    break;
                }
            }
        }
    }

    Ok(())
}

fn find_uninstalled_games(
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
) -> Result<Vec<String>> {
    let installed_games: HashSet<_> = installations
        .installations
        .iter()
        .map(|inst| &inst.game_name.0)
        .collect();

    let uninstalled_games: Vec<String> = game_config
        .games
        .iter()
        .filter(|game| !installed_games.contains(&game.name.0))
        .map(|game| game.name.0.clone())
        .collect();

    Ok(uninstalled_games)
}
