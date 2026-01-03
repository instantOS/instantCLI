mod edit_menu;
mod editors;
mod state;

use anyhow::{Context, Result, anyhow};

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::selection::select_game_interactive;
use crate::game::operations::launch_game;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use state::EditState;

/// Game action selection
#[derive(Debug, Clone)]
enum GameAction {
    Launch,
    Edit,
}

#[derive(Clone)]
struct GameActionItem {
    display: String,
    preview: String,
    action: GameAction,
}

impl FzfSelectable for GameActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.display.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

/// Main entry point for the game menu
pub fn game_menu(provided_game_name: Option<String>) -> Result<()> {
    let has_provided_name = provided_game_name.is_some();
    
    loop {
        // Select game if not provided
        let game_name = match &provided_game_name {
            Some(name) => name.clone(),
            None => match select_game_interactive(None)? {
                Some(name) => name,
                None => return Ok(()),
            },
        };

        // Show game action menu
        let actions = vec![
            GameActionItem {
                display: format!("{} Launch", char::from(NerdFont::Rocket)),
                preview: format!("Launch {} with automatic save sync", game_name),
                action: GameAction::Launch,
            },
            GameActionItem {
                display: format!("{} Edit", char::from(NerdFont::Edit)),
                preview: format!("Edit {}'s configuration (name, description, launch command, save path)", game_name),
                action: GameAction::Edit,
            },
        ];

        let selection = FzfWrapper::builder()
            .header(format!("Game: {}", game_name))
            .prompt("Select action")
            .select(actions)?;

        match selection {
            FzfResult::Selected(item) => match item.action {
                GameAction::Launch => {
                    // Check if the game has a launch command
                    if !has_launch_command(&game_name)? {
                        FzfWrapper::message(&format!(
                            "No launch command configured for '{}'.\n\nUse Edit to set a launch command first.",
                            game_name
                        ))?;
                        continue;
                    }
                    launch_game(Some(game_name))?;
                    return Ok(());
                }
                GameAction::Edit => {
                    run_edit_menu_for_game(&game_name)?;
                    // If game_name was provided as argument, exit after edit
                    // Otherwise, loop back to game selection
                    if has_provided_name {
                        return Ok(());
                    }
                }
            },
            FzfResult::Cancelled => {
                // If game_name was provided as argument, exit
                // Otherwise, loop back to game selection
                if has_provided_name {
                    return Ok(());
                }
                // Continue loop to show game selection again
            }
            _ => return Ok(()),
        }
    }
}

/// Run the edit menu for a specific game
fn run_edit_menu_for_game(game_name: &str) -> Result<()> {
    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

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
    edit_menu::run_edit_menu(game_name, &mut state)
}

/// Check if a game has a launch command configured
fn has_launch_command(game_name: &str) -> Result<bool> {
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations = InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Find the game
    let game = game_config
        .games
        .iter()
        .find(|g| g.name.0 == game_name);

    let game_has_command = game
        .and_then(|g| g.launch_command.as_ref())
        .is_some();

    // Check installation override
    let installation_has_command = installations
        .installations
        .iter()
        .find(|i| i.game_name.0 == game_name)
        .and_then(|i| i.launch_command.as_ref())
        .is_some();

    Ok(game_has_command || installation_has_command)
}
