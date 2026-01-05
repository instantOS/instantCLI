mod edit_menu;
mod editors;
mod state;

use anyhow::{Context, Result, anyhow};

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::manager::GameManager;
use crate::game::games::manager::AddGameOptions;
use crate::game::games::selection::{GameMenuEntry, select_game_menu_entry};
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
    Move,
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
    description: Option<String>,
    save_path: Option<String>,
}

impl GameState {
    fn load(game_name: &str) -> Result<Self> {
        let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
        let installations =
            InstallationsConfig::load().context("Failed to load installations configuration")?;

        let installation = installations
            .installations
            .iter()
            .find(|i| i.game_name.0 == game_name);

        let has_installation = installation.is_some();

        let game = game_config.games.iter().find(|g| g.name.0 == game_name);
        let game_cmd = game.and_then(|g| g.launch_command.clone());
        let description = game.and_then(|g| g.description.clone());

        let inst_cmd = installation.and_then(|i| i.launch_command.clone());

        let save_path = installation.map(|i| {
            i.save_path
                .to_tilde_string()
                .unwrap_or_else(|_| i.save_path.as_path().to_string_lossy().to_string())
        });

        // Installation command takes precedence over game command
        let launch_command = inst_cmd.or(game_cmd);

        Ok(Self {
            game_config,
            installations,
            needs_setup: !has_installation,
            launch_command,
            description,
            save_path,
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
            "Launch {} with automatic save sync.\n\n{} No launch command configured. Use Edit to set one.",
            game_name,
            char::from(NerdFont::Warning)
        ),
    };

    actions.push(GameActionItem {
        display: format!("{} Launch", char::from(NerdFont::Rocket)),
        preview: launch_preview,
        action: GameAction::Launch,
    });

    // Build Edit preview showing current config
    let edit_preview = build_edit_preview(game_name, state);

    actions.push(GameActionItem {
        display: format!("{} Edit", char::from(NerdFont::Edit)),
        preview: edit_preview,
        action: GameAction::Edit,
    });

    // Move option - only show if game has a save path configured
    if !state.needs_setup {
        let move_preview = format!(
            "Move '{}' save location to a new path.\n\nCurrent path: {}",
            game_name,
            state.save_path.as_deref().unwrap_or("<not set>")
        );

        actions.push(GameActionItem {
            display: format!("{} Move", char::from(NerdFont::Folder)),
            preview: move_preview,
            action: GameAction::Move,
        });
    }

    actions.push(GameActionItem {
        display: format!("{} Back", char::from(NerdFont::ArrowLeft)),
        preview: "Return to game selection".to_string(),
        action: GameAction::Back,
    });

    actions
}

/// Build preview text for Edit option showing current config
fn build_edit_preview(game_name: &str, state: &GameState) -> String {
    let mut lines = vec![format!("Edit configuration for '{}'", game_name)];
    lines.push(String::new());

    // games.toml section
    lines.push(format!("{} games.toml:", char::from(NerdFont::FileConfig)));
    lines.push(format!(
        "  {} Description: {}",
        char::from(NerdFont::Info),
        state.description.as_deref().unwrap_or("<not set>")
    ));
    lines.push(format!(
        "  {} Launch Command: {}",
        char::from(NerdFont::Rocket),
        state.launch_command.as_deref().unwrap_or("<not set>")
    ));

    // installations.toml section
    lines.push(String::new());
    if let Some(path) = &state.save_path {
        lines.push(format!("{} installations.toml:", char::from(NerdFont::Desktop)));
        lines.push(format!(
            "  {} Save Path: {}",
            char::from(NerdFont::Folder),
            path
        ));
    } else {
        lines.push(format!(
            "{} No installation on this device",
            char::from(NerdFont::Warning)
        ));
    }

    lines.join("\n")
}

/// Result of handling a game action
enum ActionResult {
    Stay, // Stay in this game's action menu
    Back, // Go back to game selection
    Exit, // Exit the menu entirely
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
            if exit_after {
                Ok(ActionResult::Exit)
            } else {
                Ok(ActionResult::Stay)
            }
        }
        GameAction::Setup => {
            setup::setup_uninstalled_games()?;
            if exit_after {
                Ok(ActionResult::Exit)
            } else {
                Ok(ActionResult::Stay)
            }
        }
        GameAction::Move => {
            GameManager::move_game(Some(game_name.to_string()), None)?;
            if exit_after {
                Ok(ActionResult::Exit)
            } else {
                Ok(ActionResult::Stay)
            }
        }
        GameAction::Back => {
            if exit_after {
                Ok(ActionResult::Exit)
            } else {
                Ok(ActionResult::Back)
            }
        }
    }
}

/// Main entry point for the game menu
pub fn game_menu(provided_game_name: Option<String>) -> Result<()> {
    // Check if game manager is initialized
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;
    if !config.is_initialized() {
        // Show simplified menu with initialization option
        return show_uninitialized_menu();
    }

    let exit_after_action = provided_game_name.is_some();

    // If a game name is provided, skip the menu and go directly to actions
    if let Some(name) = &provided_game_name {
        loop {
            let state = GameState::load(name)?;
            let actions = build_action_menu(name, &state);

            let selection = FzfWrapper::builder()
                .header(format!("Game: {}", name))
                .prompt("Select action")
                .select(actions)?;

            let result = match selection {
                FzfResult::Selected(item) => {
                    handle_action(item.action, name, &state, exit_after_action)?
                }
                FzfResult::Cancelled => {
                    if exit_after_action {
                        ActionResult::Exit
                    } else {
                        ActionResult::Back
                    }
                }
                _ => ActionResult::Exit,
            };

            match result {
                ActionResult::Stay => continue,
                ActionResult::Back => break,
                ActionResult::Exit => return Ok(()),
            }
        }
        return Ok(());
    }

    // Outer loop: menu entry selection
    loop {
        let entry = match select_game_menu_entry()? {
            Some(entry) => entry,
            None => return Ok(()),
        };

        match entry {
            GameMenuEntry::AddGame => {
                GameManager::add_game(AddGameOptions::default())?;
                // Return to menu after adding
                continue;
            }
            GameMenuEntry::SetupGames => {
                setup::setup_uninstalled_games()?;
                // Return to menu after setup
                continue;
            }
            GameMenuEntry::Game(game_name) => {
                // Inner loop: game action menu
                loop {
                    let state = GameState::load(&game_name)?;
                    let actions = build_action_menu(&game_name, &state);

                    let selection = FzfWrapper::builder()
                        .header(format!("Game: {}", game_name))
                        .prompt("Select action")
                        .select(actions)?;

                    let result = match selection {
                        FzfResult::Selected(item) => {
                            handle_action(item.action, &game_name, &state, false)?
                        }
                        FzfResult::Cancelled => ActionResult::Back,
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

/// Show menu when game manager is not initialized
fn show_uninitialized_menu() -> Result<()> {
    let options = vec![
        format!("{} Initialize Game Manager", char::from(NerdFont::Play)),
        format!("{} Cancel", char::from(NerdFont::CrossCircle)),
    ];

    let selection = FzfWrapper::builder()
        .header("Game save manager is not initialized")
        .prompt("Select action")
        .select(options)?;

    match selection {
        FzfResult::Selected(item) if item == format!("{} Initialize Game Manager", char::from(NerdFont::Play)) => {
            // Run initialization
            let init_result = crate::game::repository::manager::RepositoryManager::initialize_game_manager(false, Default::default());
            match init_result {
                Ok(()) => {
                    FzfWrapper::message("Game save manager initialized successfully!\n\nYou can now add games with 'ins game add' or open the menu again.")?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Initialization failed: {}", e))?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
