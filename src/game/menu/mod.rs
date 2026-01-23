mod edit_menu;
mod editors;
mod state;

use anyhow::{Context, Result, anyhow};

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::manager::AddGameOptions;
use crate::game::games::manager::GameManager;
use crate::game::games::selection::{GameMenuEntry, select_game_menu_entry};
use crate::game::operations::launch_game;
use crate::game::restic;
use crate::game::setup;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use state::EditState;

/// Game action selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameAction {
    Launch,
    Edit,
    Setup,
    Move,
    Checkpoint,
    Back,
}

#[derive(Clone)]
struct GameActionItem {
    display: String,
    action: GameAction,
    preview: FzfPreview,
}

impl FzfSelectable for GameActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        match self.action {
            GameAction::Launch => "launch".to_string(),
            GameAction::Edit => "edit".to_string(),
            GameAction::Setup => "setup".to_string(),
            GameAction::Move => "move".to_string(),
            GameAction::Checkpoint => "checkpoint".to_string(),
            GameAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
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
}

/// Build the game action menu items
fn build_action_menu(game_name: &str, state: &GameState) -> Vec<GameActionItem> {
    let mut actions = Vec::new();

    // Add setup option first if game needs it
    if state.needs_setup {
        actions.push(GameActionItem {
            display: format!(
                "{} Setup",
                format_icon_colored(NerdFont::Wrench, colors::PEACH)
            ),
            action: GameAction::Setup,
            preview: PreviewBuilder::new()
                .header(NerdFont::Wrench, "Setup Game")
                .text(&format!(
                    "Configure save path for '{}' on this device.",
                    game_name
                ))
                .blank()
                .text("This game is registered but has no save data location set up yet.")
                .build(),
        });
    }

    // Launch preview
    let launch_preview = {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Rocket, "Launch Game")
            .text(&format!("Launch {} with automatic save sync.", game_name));

        if let Some(cmd) = &state.launch_command {
            builder = builder.blank().field("Command", cmd);
        } else {
            builder = builder.blank().line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "No launch command configured. Use Edit to set one.",
            );
        }
        builder.build()
    };

    actions.push(GameActionItem {
        display: format!(
            "{} Launch",
            format_icon_colored(NerdFont::Rocket, colors::GREEN)
        ),
        action: GameAction::Launch,
        preview: launch_preview,
    });

    // Edit preview
    let edit_preview = {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Edit, "Edit Configuration")
            .text(&format!("Edit configuration for '{}'", game_name))
            .blank()
            .separator()
            .blank()
            .line(colors::MAUVE, Some(NerdFont::FileConfig), "games.toml")
            .field_indented(
                "Description",
                state.description.as_deref().unwrap_or("<not set>"),
            )
            .field_indented(
                "Launch Command",
                state.launch_command.as_deref().unwrap_or("<not set>"),
            );

        if let Some(path) = &state.save_path {
            builder = builder
                .blank()
                .line(colors::MAUVE, Some(NerdFont::Desktop), "installations.toml")
                .field_indented("Save Path", path);
        } else {
            builder = builder.blank().subtext("No installation on this device");
        }
        builder.build()
    };

    actions.push(GameActionItem {
        display: format!(
            "{} Edit",
            format_icon_colored(NerdFont::Edit, colors::SAPPHIRE)
        ),
        action: GameAction::Edit,
        preview: edit_preview,
    });

    // Move option - only show if game has a save path configured
    if !state.needs_setup {
        if let Some(path) = &state.save_path {
            actions.push(GameActionItem {
                display: format!(
                    "{} Move",
                    format_icon_colored(NerdFont::Folder, colors::LAVENDER)
                ),
                action: GameAction::Move,
                preview: PreviewBuilder::new()
                    .header(NerdFont::Folder, "Move Save Location")
                    .text(&format!(
                        "Move '{}' save location to a new path.",
                        game_name
                    ))
                    .blank()
                    .field("Current path", path)
                    .build(),
            });
        }

        actions.push(GameActionItem {
            display: format!(
                "{} Checkpoint",
                format_icon_colored(NerdFont::Clock, colors::YELLOW)
            ),
            action: GameAction::Checkpoint,
            preview: PreviewBuilder::new()
                .header(NerdFont::Clock, "Restore Checkpoint")
                .text(&format!(
                    "Browse and restore from a past checkpoint for '{}'.",
                    game_name
                ))
                .blank()
                .text("View all available snapshots and select one to restore.")
                .text("If local saves are newer, you will be warned before overwriting.")
                .build(),
        });
    }

    actions.push(GameActionItem {
        display: format!("{} Back", format_back_icon()),
        action: GameAction::Back,
        preview: PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to game selection.")
            .build(),
    });

    actions
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
            if state.launch_command.is_none() {
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
        GameAction::Checkpoint => {
            handle_checkpoint_action(game_name)?;
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

/// Handle checkpoint action - select and restore from a past snapshot
fn handle_checkpoint_action(game_name: &str) -> Result<()> {
    use crate::game::restic::snapshot_selection::select_snapshot_interactive_with_local_comparison;

    // Get installation for local save comparison
    let installations = InstallationsConfig::load().context("Failed to load installations")?;
    let installation = installations
        .installations
        .iter()
        .find(|i| i.game_name.0 == game_name);

    // Select a snapshot interactively
    let snapshot_id =
        match select_snapshot_interactive_with_local_comparison(game_name, installation)? {
            Some(id) => id,
            None => return Ok(()), // User cancelled
        };

    // Restore the selected snapshot
    restic::restore_game_saves(Some(game_name.to_string()), Some(snapshot_id), false)
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
        let mut cursor = MenuCursor::new();

        loop {
            let state = GameState::load(name)?;
            let actions = build_action_menu(name, &state);

            let mut builder = FzfWrapper::builder()
                .header(Header::fancy(&format!("Game: {}", name)))
                .prompt("Select action")
                .args(fzf_mocha_args())
                .responsive_layout();

            if let Some(index) = cursor.initial_index(&actions) {
                builder = builder.initial_index(index);
            }

            let selection = builder.select_padded(actions.clone())?;

            let result = match selection {
                FzfResult::Selected(item) => {
                    cursor.update(&item, &actions);
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
    let mut cursor = MenuCursor::new();
    loop {
        let entry = match select_game_menu_entry(&mut cursor)? {
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
                let mut cursor = MenuCursor::new();
                loop {
                    let state = GameState::load(&game_name)?;
                    let actions = build_action_menu(&game_name, &state);

                    let mut builder = FzfWrapper::builder()
                        .header(Header::fancy(&format!("Game: {}", game_name)))
                        .prompt("Select action")
                        .args(fzf_mocha_args())
                        .responsive_layout();

                    if let Some(index) = cursor.initial_index(&actions) {
                        builder = builder.initial_index(index);
                    }

                    let selection = builder.select_padded(actions.clone())?;

                    let result = match selection {
                        FzfResult::Selected(item) => {
                            cursor.update(&item, &actions);
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
            GameMenuEntry::CloseMenu => {
                // Exit the menu entirely
                return Ok(());
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
        format!(
            "{} Initialize Game Manager",
            format_icon_colored(NerdFont::Play, colors::GREEN)
        ),
        format!("{} Cancel", format_back_icon()),
    ];

    let selection = FzfWrapper::builder()
        .header(Header::fancy("Game save manager is not initialized"))
        .prompt("Select action")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_padded(options)?;

    match selection {
        FzfResult::Selected(item)
            if item
                == format!(
                    "{} Initialize Game Manager",
                    format_icon_colored(NerdFont::Play, colors::GREEN)
                ) =>
        {
            // Run initialization
            let init_result =
                crate::game::repository::manager::RepositoryManager::initialize_game_manager(
                    false,
                    Default::default(),
                );
            match init_result {
                Ok(()) => {
                    FzfWrapper::message(
                        "Game save manager initialized successfully!\n\nYou can now add games with 'ins game add' or open the menu again.",
                    )?;
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
