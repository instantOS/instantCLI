mod edit_menu;
mod editors;
mod state;

use anyhow::{Context, Result, anyhow};

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::selection::select_game_interactive;
use crate::game::operations::launch_game;
use crate::game::setup;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use state::EditState;

/// Game action selection
#[derive(Debug, Clone)]
enum GameAction {
    Launch,
    Edit,
    Setup,
    Back,
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

/// Loaded game state to avoid repeated config loading
struct GameState {
    game_config: InstantGameConfig,
    installations: InstallationsConfig,
    needs_setup: bool,
    launch_command: Option<String>,
}

impl GameState {
    fn load(game_name: &str) -> Result<Self> {
        let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
        let installations = InstallationsConfig::load().context("Failed to load installations configuration")?;

        let has_installation = installations
            .installations
            .iter()
            .any(|i| i.game_name.0 == game_name);

        let game = game_config.games.iter().find(|g| g.name.0 == game_name);
        let game_cmd = game.and_then(|g| g.launch_command.clone());

        let inst_cmd = installations
            .installations
            .iter()
            .find(|i| i.game_name.0 == game_name)
            .and_then(|i| i.launch_command.clone());

        // Installation command takes precedence over game command
        let launch_command = inst_cmd.or(game_cmd);

        Ok(Self {
            game_config,
            installations,
            needs_setup: !has_installation,
            launch_command,
        })
    }

    fn has_launch_command(&self) -> bool {
        self.launch_command.is_some()
    }
}

/// Build the game action menu items
fn build_action_menu(game_name: &str, state: &GameState) -> Vec<GameActionItem> {
    let mut actions = Vec::new();

    // Add setup option first if game needs it
    if state.needs_setup {
        actions.push(GameActionItem {
            display: format!("{} Setup", char::from(NerdFont::Wrench)),
            preview: format!(
                "Configure save path for '{}' on this device.\n\nThis game is registered but has no save data location set up yet.",
                game_name
            ),
            action: GameAction::Setup,
        });
    }

    let launch_preview = match &state.launch_command {
        Some(cmd) => format!(
            "Launch {} with automatic save sync.\n\nCommand: {}",
            game_name, cmd
        ),
        None => format!(
            "Launch {} with automatic save sync.\n\n⚠ No launch command configured. Use Edit to set one.",
            game_name
        ),
    };

    actions.push(GameActionItem {
        display: format!("{} Launch", char::from(NerdFont::Rocket)),
        preview: launch_preview,
        action: GameAction::Launch,
    });

    actions.push(GameActionItem {
        display: format!("{} Edit", char::from(NerdFont::Edit)),
        preview: format!("Edit {}'s configuration (name, description, launch command, save path)", game_name),
        action: GameAction::Edit,
    });

    actions.push(GameActionItem {
        display: format!("{} Back", char::from(NerdFont::ArrowLeft)),
        preview: "Return to game selection".to_string(),
        action: GameAction::Back,
    });

    actions
}

/// Result of handling a game action
enum ActionResult {
    Stay,       // Stay in this game's action menu
    Back,       // Go back to game selection
    Exit,       // Exit the menu entirely
}

/// Handle a selected game action
fn handle_action(
    action: GameAction,
    game_name: &str,
    state: &GameState,
    exit_after: bool,
) -> Result<ActionResult> {
    match action {
        GameAction::Launch => {
            if !state.has_launch_command() {
                FzfWrapper::message(&format!(
                    "No launch command configured for '{}'.\n\nUse Edit to set a launch command first.",
                    game_name
                ))?;
                return Ok(ActionResult::Stay);
            }
            if state.needs_setup {
                FzfWrapper::message(&format!(
                    "No save path configured for '{}' on this device.\n\nUse Setup to configure the save path first.",
                    game_name
                ))?;
                return Ok(ActionResult::Stay);
            }
            launch_game(Some(game_name.to_string()))?;
            Ok(ActionResult::Exit)
        }
        GameAction::Edit => {
            run_edit_menu_for_game(game_name, &state.game_config, &state.installations)?;
            if exit_after { Ok(ActionResult::Exit) } else { Ok(ActionResult::Stay) }
        }
        GameAction::Setup => {
            setup::setup_uninstalled_games()?;
            if exit_after { Ok(ActionResult::Exit) } else { Ok(ActionResult::Stay) }
        }
        GameAction::Back => {
            if exit_after { Ok(ActionResult::Exit) } else { Ok(ActionResult::Back) }
        }
    }
}

/// Main entry point for the game menu
pub fn game_menu(provided_game_name: Option<String>) -> Result<()> {
    let exit_after_action = provided_game_name.is_some();

    // Outer loop: game selection
    loop {
        let game_name = match &provided_game_name {
            Some(name) => name.clone(),
            None => match select_game_interactive(None)? {
                Some(name) => name,
                None => return Ok(()),
            },
        };

        // Inner loop: game action menu
        loop {
            let state = GameState::load(&game_name)?;
            let actions = build_action_menu(&game_name, &state);

            let selection = FzfWrapper::builder()
                .header(format!("Game: {}", game_name))
                .prompt("Select action")
                .select(actions)?;

            let result = match selection {
                FzfResult::Selected(item) => handle_action(item.action, &game_name, &state, exit_after_action)?,
                FzfResult::Cancelled => {
                    if exit_after_action { ActionResult::Exit } else { ActionResult::Back }
                }
                _ => ActionResult::Exit,
            };

            match result {
                ActionResult::Stay => continue,
                ActionResult::Back => break,
                ActionResult::Exit => return Ok(()),
            }
        }
    }
}

/// Run the edit menu for a specific game
fn run_edit_menu_for_game(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
) -> Result<()> {
    let game_index = game_config
        .games
        .iter()
        .position(|g| g.name.0 == game_name)
        .ok_or_else(|| anyhow!("Game '{}' not found in games.toml", game_name))?;

    let installation_index = installations
        .installations
        .iter()
        .position(|i| i.game_name.0 == game_name);

    let mut state = EditState::new(
        game_config.clone(),
        installations.clone(),
        game_index,
        installation_index,
    );
    edit_menu::run_edit_menu(game_name, &mut state)
}
