use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::selection::select_game_interactive;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use anyhow::{Context, Result, anyhow};

pub(super) fn remove_game(game_name: Option<String>, force: bool) -> Result<()> {
    let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    let game_name = match game_name {
        Some(name) => name,
        None => match select_game_interactive(Some("Select a game to remove:"))? {
            Some(name) => name,
            None => return Ok(()),
        },
    };

    let game_index = config.games.iter().position(|g| g.name.0 == game_name);

    if game_index.is_none() {
        eprintln!("Game '{game_name}' not found in configuration.");
        return Ok(());
    }

    let game_index = game_index.unwrap();

    if force {
        remove_game_entry(&mut config, &mut installations, game_index, &game_name)?;
        println!("✓ Game '{game_name}' removed successfully!");
        return Ok(());
    }

    let game = &config.games[game_index];

    match FzfWrapper::builder()
        .confirm(format!(
            "Are you sure you want to remove the following game?\n\n\
             Game: {}\n\
             Description: {}\n\
             Launch command: {}\n\n\
             This will remove the game from your configuration and save path mapping.",
            game.name.0,
            game.description.as_deref().unwrap_or("None"),
            game.launch_command.as_deref().unwrap_or("None")
        ))
        .yes_text("Remove Game")
        .no_text("Keep Game")
        .show_confirmation()
        .map_err(|e| anyhow!("Failed to get confirmation: {}", e))?
    {
        ConfirmResult::Yes => {
            remove_game_entry(&mut config, &mut installations, game_index, &game_name)?;
            println!("✓ Game '{game_name}' removed successfully!");
        }
        ConfirmResult::No | ConfirmResult::Cancelled => {
            println!("Game removal cancelled.");
        }
    }

    Ok(())
}

fn remove_game_entry(
    config: &mut InstantGameConfig,
    installations: &mut InstallationsConfig,
    game_index: usize,
    game_name: &str,
) -> Result<()> {
    config.games.remove(game_index);
    config.save()?;

    installations
        .installations
        .retain(|inst| inst.game_name.0 != game_name);
    installations.save()?;

    Ok(())
}
