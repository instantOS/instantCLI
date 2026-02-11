mod edit_menu;
mod editors;
mod state;

use anyhow::{Context, Result, anyhow};

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::AddGameOptions;
use crate::game::games::GameManager;
use crate::game::games::selection::{GameMenuEntry, select_game_menu_entry};
use crate::game::operations::desktop;
use crate::game::operations::launch_game;
use crate::game::operations::steam;
use crate::game::operations::sync::sync_game_saves;
use crate::game::restic;
use crate::game::setup;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
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
    AddToSteam,
    AddToDesktop,
    Back,
}

impl std::fmt::Display for GameAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameAction::Launch => write!(f, "launch"),
            GameAction::Edit => write!(f, "edit"),
            GameAction::Setup => write!(f, "setup"),
            GameAction::Move => write!(f, "move"),
            GameAction::Checkpoint => write!(f, "checkpoint"),
            GameAction::AddToSteam => write!(f, "add-to-steam"),
            GameAction::AddToDesktop => write!(f, "add-to-desktop"),
            GameAction::Back => write!(f, "back"),
        }
    }
}

#[derive(Clone)]
struct GameActionItem {
    display: String,
    action: GameAction,
    preview: FzfPreview,
    keywords: Vec<&'static str>,
}

impl FzfSelectable for GameActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.action.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_search_keywords(&self) -> &[&str] {
        &self.keywords
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
            keywords: vec![],
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
            builder = builder
                .blank()
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Warning),
                    "No launch command configured.",
                )
                .subtext("Pressing Launch will offer to build one.");
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
        keywords: vec!["play", "start", "run"],
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
        keywords: vec!["change", "modify", "configure", "settings"],
    });

    // Move option - only show if game has a save path configured
    if !state.needs_setup {
        if let Some(path) = &state.save_path {
            actions.push(GameActionItem {
                display: format!(
                    "{} Change Save Path",
                    format_icon_colored(NerdFont::Folder, colors::LAVENDER)
                ),
                action: GameAction::Move,
                preview: PreviewBuilder::new()
                    .header(NerdFont::Folder, "Change Save Path")
                    .text(&format!("Select a new save location for '{}'.", game_name))
                    .blank()
                    .subtext("This updates where the game manager looks for saves.")
                    .subtext("Files are not moved automatically.")
                    .blank()
                    .field("Current path", path)
                    .build(),
                keywords: vec![],
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
            keywords: vec!["snapshot"],
        });
    }

    if state.launch_command.is_some() {
        let is_in_steam = steam::is_game_in_steam(game_name).unwrap_or(false);
        let status_text = if is_in_steam {
            format!(
                "{} Already in Steam Library",
                format_icon_colored(NerdFont::Check, colors::GREEN)
            )
        } else {
            format!(
                "{} Not in Steam Library",
                format_icon_colored(NerdFont::Cross, colors::RED)
            )
        };

        // Build preview with command details if already in Steam
        let mut preview_builder = PreviewBuilder::new()
            .header(NerdFont::Steam, "Add to Steam")
            .text(&format!("Manage Steam shortcut for '{}'.", game_name))
            .blank()
            .text(&status_text);

        // Add command details if game is already in Steam
        if is_in_steam && let Ok(Some(shortcut)) = steam::get_game_shortcut(game_name) {
            preview_builder = preview_builder.blank().text("Current shortcut details:");

            preview_builder = preview_builder
                .subtext(&format!("Exe:        {}", shortcut.exe))
                .subtext(&format!("Start dir:  {}", shortcut.start_dir));

            if !shortcut.launch_options.is_empty() {
                preview_builder =
                    preview_builder.subtext(&format!("Options:    {}", shortcut.launch_options));
            }

            // Show the full command as Steam would run it
            let full_cmd = if shortcut.launch_options.is_empty() {
                shortcut.exe.clone()
            } else {
                format!("{} {}", shortcut.exe, shortcut.launch_options)
            };
            preview_builder = preview_builder
                .blank()
                .subtext(&format!("Full command: {}", full_cmd));
        }

        let preview = preview_builder
            .blank()
            .text("The shortcut will launch the game through ins,")
            .text("which automatically syncs saves before and after.")
            .blank()
            .subtext("Note: Restart Steam after adding to see the shortcut.")
            .build();

        actions.push(GameActionItem {
            display: format!(
                "{} Add to Steam",
                format_icon_colored(NerdFont::Steam, colors::SAPPHIRE)
            ),
            action: GameAction::AddToSteam,
            preview,
            keywords: vec![],
        });

        // Add desktop shortcut option
        let is_on_desktop = desktop::is_game_on_desktop(game_name).unwrap_or(false);
        let desktop_status_text = if is_on_desktop {
            format!(
                "{} Already on Desktop",
                format_icon_colored(NerdFont::Check, colors::GREEN)
            )
        } else {
            format!(
                "{} Not on Desktop",
                format_icon_colored(NerdFont::Cross, colors::RED)
            )
        };

        // Build preview with path details if already on desktop
        let mut desktop_preview_builder = PreviewBuilder::new()
            .header(NerdFont::Desktop, "Add to Desktop")
            .text(&format!("Manage desktop shortcut for '{}'.", game_name))
            .blank()
            .text(&desktop_status_text);

        // Add path details if game is already on desktop
        if is_on_desktop && let Ok(Some(path)) = desktop::get_game_desktop_path(game_name) {
            desktop_preview_builder = desktop_preview_builder
                .blank()
                .text("Current shortcut location:")
                .subtext(&path.display().to_string());
        }

        let desktop_preview = desktop_preview_builder
            .blank()
            .text("Creates a .desktop file that launches the game through ins,")
            .text("which automatically syncs saves before and after.")
            .blank()
            .subtext("The shortcut will be placed on your Desktop if possible,")
            .subtext("otherwise in the applications menu.")
            .build();

        actions.push(GameActionItem {
            display: format!(
                "{} Add to Desktop",
                format_icon_colored(NerdFont::Desktop, colors::MAUVE)
            ),
            action: GameAction::AddToDesktop,
            preview: desktop_preview,
            keywords: vec!["shortcut", "launcher"],
        });
    }

    actions.push(GameActionItem {
        display: format!("{} Back", format_back_icon()),
        action: GameAction::Back,
        preview: PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to game selection.")
            .build(),
        keywords: vec![],
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
                // Show builder menu directly to let user choose manual or builder
                match crate::game::launch_builder::build_launch_command()? {
                    Some(command) => {
                        // Save to game config
                        let mut game_config = state.game_config.clone();
                        if let Some(game) =
                            game_config.games.iter_mut().find(|g| g.name.0 == game_name)
                        {
                            game.launch_command = Some(command.clone());
                            game_config.save()?;
                            FzfWrapper::message(&format!(
                                "{} Launch command saved. Launching game now...",
                                char::from(NerdFont::Check)
                            ))?;
                        } else {
                            return Ok(ActionResult::Stay);
                        }
                    }
                    None => return Ok(ActionResult::Stay),
                }
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
            GameManager::relocate_game(Some(game_name.to_string()), None)?;
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
        GameAction::AddToSteam => {
            let launch_cmd = state.launch_command.as_deref().unwrap_or("");

            if steam::is_game_in_steam(game_name).unwrap_or(false) {
                if FzfWrapper::builder()
                    .confirm(format!(
                        "'{}' is already in Steam.\n\nRemove it?",
                        game_name
                    ))
                    .yes_text("Remove from Steam")
                    .no_text("Keep it")
                    .confirm_dialog()?
                    == ConfirmResult::Yes
                {
                    match steam::remove_game_from_steam(game_name) {
                        Ok((true, steam_running)) => {
                            let base_msg = format!("Removed '{}' from Steam.", game_name);
                            let msg = if steam_running {
                                format!(
                                    "{}\n\n{}",
                                    base_msg,
                                    steam::format_steam_running_warning("removed")
                                )
                            } else {
                                format!("{}\n\nRestart Steam to update your library.", base_msg)
                            };
                            FzfWrapper::message(&msg)?;
                        }
                        Ok((false, _)) => FzfWrapper::message(&format!(
                            "'{}' was not found in Steam (maybe already removed).",
                            game_name
                        ))?,
                        Err(e) => {
                            FzfWrapper::message(&format!("Failed to remove from Steam: {}", e))?
                        }
                    }
                }
            } else {
                match steam::add_game_to_steam(game_name, launch_cmd) {
                    Ok((true, steam_running)) => {
                        let base_msg =
                            format!("Added '{}' to Steam as a non-Steam game.", game_name);
                        let msg = if steam_running {
                            format!(
                                "{}\n\n{}",
                                base_msg,
                                steam::format_steam_running_warning("added")
                            )
                        } else {
                            format!("{}\n\nRestart Steam to see it in your library.", base_msg)
                        };
                        FzfWrapper::message(&msg)?;
                    }
                    Ok((false, _)) => {
                        FzfWrapper::message(&format!(
                            "'{}' is already in your Steam library.",
                            game_name
                        ))?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Failed to add to Steam: {}", e))?;
                    }
                }
            }
            Ok(ActionResult::Stay)
        }
        GameAction::AddToDesktop => {
            let launch_cmd = state.launch_command.as_deref().unwrap_or("");

            if desktop::is_game_on_desktop(game_name).unwrap_or(false) {
                if FzfWrapper::builder()
                    .confirm(format!(
                        "'{}' is already on the Desktop.\n\nRemove it?",
                        game_name
                    ))
                    .yes_text("Remove from Desktop")
                    .no_text("Keep it")
                    .confirm_dialog()?
                    == ConfirmResult::Yes
                {
                    match desktop::remove_game_from_desktop(game_name) {
                        Ok(true) => {
                            FzfWrapper::message(&format!("Removed '{}' from Desktop.", game_name))?;
                        }
                        Ok(false) => FzfWrapper::message(&format!(
                            "'{}' was not found on the Desktop (maybe already removed).",
                            game_name
                        ))?,
                        Err(e) => {
                            FzfWrapper::message(&format!("Failed to remove from Desktop: {}", e))?
                        }
                    }
                }
            } else {
                match desktop::add_game_to_desktop(game_name, launch_cmd) {
                    Ok((true, Some(path))) => {
                        FzfWrapper::message(&format!(
                            "Added '{}' to Desktop.\n\nLocation: {}",
                            game_name,
                            path.display()
                        ))?;
                    }
                    Ok((true, None)) => {
                        FzfWrapper::message(&format!("Added '{}' to Desktop.", game_name))?;
                    }
                    Ok((false, _)) => {
                        FzfWrapper::message(&format!(
                            "'{}' is already on your Desktop.",
                            game_name
                        ))?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Failed to add to Desktop: {}", e))?;
                    }
                }
            }
            Ok(ActionResult::Stay)
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
    use crate::game::restic::snapshot_selection::select_snapshot_interactive;

    // Get installation for local save comparison
    let installations = InstallationsConfig::load().context("Failed to load installations")?;
    let installation = installations
        .installations
        .iter()
        .find(|i| i.game_name.0 == game_name);

    // Select a snapshot interactively
    let snapshot_id = match select_snapshot_interactive(game_name, installation)? {
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
            GameMenuEntry::SyncAll => {
                match sync_game_saves(None, false) {
                    Ok(summary) => {
                        // Show summary in a message dialog
                        let message = if summary.total() == 0 {
                            "No games configured for syncing.".to_string()
                        } else {
                            format!(
                                "{} Sync Summary\n\n✅ Synced: {}\n🔶 Skipped: {}\n❌ Errors: {}",
                                char::from(NerdFont::Chart),
                                summary.synced,
                                summary.skipped,
                                summary.errors
                            )
                        };

                        FzfWrapper::message(&message)?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Sync failed: {}", e))?;
                    }
                }
                // Return to menu after user dismisses the message
                continue;
            }
            GameMenuEntry::AddMenuToSteam => {
                match steam::add_game_menu_to_steam() {
                    Ok((true, steam_running)) => {
                        let base_msg = "Added 'ins game menu' to Steam.";
                        let msg = if steam_running {
                            format!(
                                "{}\n\n{}",
                                base_msg,
                                steam::format_steam_running_warning("added")
                            )
                        } else {
                            format!("{}\n\nRestart Steam to see it in your library.", base_msg)
                        };
                        FzfWrapper::message(&msg)?;
                    }
                    Ok((false, _)) => {
                        FzfWrapper::message("'ins game menu' is already in your Steam library.")?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Failed to add to Steam: {}", e))?;
                    }
                }
                continue;
            }
            GameMenuEntry::AddMenuToDesktop => {
                match desktop::add_menu_to_desktop() {
                    Ok((true, Some(path))) => {
                        FzfWrapper::message(&format!(
                            "Added 'ins game menu' to Desktop.\n\nLocation: {}",
                            path.display()
                        ))?;
                    }
                    Ok((true, None)) => {
                        FzfWrapper::message("Added 'ins game menu' to Desktop.")?;
                    }
                    Ok((false, _)) => {
                        FzfWrapper::message("'ins game menu' is already on your Desktop.")?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Failed to add to Desktop: {}", e))?;
                    }
                }
                continue;
            }
            GameMenuEntry::Game(game_name, _) => {
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
                crate::game::repository::manager::GameRepositoryManager::initialize_game_manager(
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
