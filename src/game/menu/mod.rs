mod action_menu;
mod actions;
mod edit_menu;
mod editors;
mod state;

use anyhow::{Context, Result, anyhow};

use crate::common::TildePath;
use crate::game::GameCommands;
use crate::game::config::{
    Game, GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind,
};
use crate::game::games::AddGameOptions;
use crate::game::games::selection::{GameMenuEntry, select_game_menu_entry};
use crate::game::handle_game_command;
use crate::game::launch_command::LaunchCommand;
use crate::game::operations::sync::sync_game_saves;
use crate::game::operations::{desktop, steam};
use crate::game::setup;
use crate::menu_utils::{FzfWrapper, Header, HeaderBuilder, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;

use action_menu::build_action_menu;
use actions::{ActionResult, handle_action};
use state::{EditState, GameState};

pub fn game_menu(provided_game_name: Option<String>) -> Result<()> {
    // Check if game manager is initialized
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;
    if !config.is_initialized() {
        // Show simplified menu with initialization option
        return show_uninitialized_menu();
    }

    if let Some(name) = provided_game_name {
        run_game_action_menu(&name, true)?;
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
                handle_game_command(
                    GameCommands::Add {
                        save_group: None,
                        name: None,
                        description: None,
                        launch_command: None,
                        save_path: None,
                        create_save_path: false,
                        skip_save_setup: false,
                        no_cache: false,
                    },
                    crate::ui::is_debug_enabled(),
                )?;
                // Return to menu after adding
                continue;
            }
            GameMenuEntry::SetupGames => {
                setup::setup_uninstalled_games()?;
                // Return to menu after setup
                continue;
            }
            GameMenuEntry::SyncAll => {
                match sync_game_saves(None, false, &mut |_| {}) {
                    Ok(report) => {
                        let summary = report.summary;
                        // Show summary in a message dialog
                        let message = if summary.total() == 0 {
                            "No games configured for syncing.".to_string()
                        } else {
                            format!(
                                "{} Sync Summary\n\n{} Backed up: {}\n{} Restored: {}\n{} Skipped: {}\n{} Errors: {}",
                                char::from(NerdFont::Chart),
                                char::from(NerdFont::Upload),
                                summary.backed_up,
                                char::from(NerdFont::Download),
                                summary.restored,
                                char::from(NerdFont::Flag),
                                summary.skipped,
                                char::from(NerdFont::CrossCircle),
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
                if let ActionResult::Exit = run_game_action_menu(&game_name, false)? {
                    return Ok(());
                }
            }
            GameMenuEntry::CloseMenu => {
                // Exit the menu entirely
                return Ok(());
            }
        }
    }
}

/// Run the same action flow for direct invocation and selection from the game list.
fn run_game_action_menu(game_name: &str, exit_after_action: bool) -> Result<ActionResult> {
    let mut cursor = MenuCursor::new();
    loop {
        let state = GameState::load(game_name)?;
        let actions = build_action_menu(game_name, &state);
        let mut builder = FzfWrapper::builder()
            .header(HeaderBuilder::new(NerdFont::Gamepad, game_name).build())
            .prompt("Select action")
            .responsive_layout();
        if let Some(index) = cursor.initial_index(&actions) {
            builder = builder.initial_index(index);
        }
        let selection = builder.items(actions.clone()).padded().select_one()?;
        let result = match selection {
            crate::menu_utils::DialogOutcome::Submitted(item) => {
                cursor.update(&item, &actions);
                handle_action(item.action, game_name, &state, exit_after_action)?
            }
            crate::menu_utils::DialogOutcome::Cancelled => {
                if exit_after_action {
                    ActionResult::Exit
                } else {
                    ActionResult::Back
                }
            }
        };
        match result {
            ActionResult::Stay => continue,
            result => return Ok(result),
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

pub fn open_prefilled_add_editor(options: AddGameOptions) -> Result<()> {
    open_prefilled_editor(options, None)
}

pub(crate) fn open_prefilled_shared_add_editor(options: AddGameOptions, group: &str) -> Result<()> {
    open_prefilled_editor(options, Some(group))
}

fn open_prefilled_editor(options: AddGameOptions, group: Option<&str>) -> Result<()> {
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;
    let mut state = prepare_prefilled_state(options, group, game_config, installations)?;
    let name = state.game().name.0.clone();
    edit_menu::run_edit_menu(&name, &mut state)
}

fn prepare_prefilled_state(
    options: AddGameOptions,
    group: Option<&str>,
    mut game_config: InstantGameConfig,
    mut installations: InstallationsConfig,
) -> Result<EditState> {
    let game_name = options.name.unwrap_or_default().trim().to_string();
    anyhow::ensure!(!game_name.is_empty(), "Discovered game is missing a name");
    anyhow::ensure!(
        !game_config
            .games
            .iter()
            .any(|game| game.name.0 == game_name),
        "Game '{game_name}' already exists"
    );
    let mut game = Game::new(game_name.clone());
    game.description = options.description.filter(|value| !value.trim().is_empty());
    game.launch_command = options
        .launch_command
        .filter(|value| !value.trim().is_empty())
        .map(LaunchCommand::from_shell_or_manual);
    let installation_index = if let Some(id) = group {
        anyhow::ensure!(
            game_config.save_groups.iter().any(|group| group.id == id),
            "Unknown save group '{id}'"
        );
        anyhow::ensure!(
            installations.save_groups.iter().any(|group| group.id == id),
            "Save group '{id}' is not bound on this device"
        );
        game.save_group = Some(id.into());
        None
    } else {
        let path = options
            .save_path
            .filter(|value| !value.trim().is_empty())
            .context("Choose a save path or shared group before adding this game")?;
        let path = TildePath::from_str(path.trim());
        let kind = std::fs::metadata(path.as_path())
            .map(PathContentKind::from)
            .context("Failed to inspect selected save path")?;
        installations
            .installations
            .push(GameInstallation::with_kind(game_name.clone(), path, kind));
        Some(installations.installations.len() - 1)
    };
    game_config.games.push(game);
    let game_index = game_config.games.len() - 1;
    let mut state = EditState::new(game_config, installations, game_index, installation_index);
    state.discovery_launch_hint = options.discovery_launch_hint;
    state.mark_dirty();
    Ok(state)
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
        .responsive_layout()
        .items(options)
        .padded()
        .select_one()?;

    match selection {
        crate::menu_utils::DialogOutcome::Submitted(item)
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
                Ok(crate::game::repository::manager::InitOutcome::Ready) => {
                    FzfWrapper::message(
                        "Backup storage is ready. Use 'ins game add' for new games or 'ins game setup' for games found in backups.",
                    )?;
                }
                Ok(crate::game::repository::manager::InitOutcome::Cancelled) => {}
                Err(e) => {
                    FzfWrapper::message(&format!("Initialization failed: {}", e))?;
                }
            }
            Ok(())
        }
        crate::menu_utils::DialogOutcome::Submitted(_)
        | crate::menu_utils::DialogOutcome::Cancelled => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_game_can_use_shared_saves_without_a_private_installation() -> Result<()> {
        use crate::game::config::{OwnedSaves, SaveGroupBinding, SharedSaveGroup};
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("shared.ps2");
        std::fs::write(&path, "card")?;
        let config = InstantGameConfig {
            save_groups: vec![SharedSaveGroup {
                id: "card".into(),
                name: "Shared card".into(),
            }],
            ..Default::default()
        };
        let local = InstallationsConfig {
            save_groups: vec![SaveGroupBinding {
                id: "card".into(),
                saves: OwnedSaves::new(TildePath::new(path), PathContentKind::File),
            }],
            ..Default::default()
        };
        let mut state = prepare_prefilled_state(
            AddGameOptions {
                name: Some("Discovered game".into()),
                launch_command: Some("pcsx2-qt -- '/games/disc.iso'".into()),
                ..Default::default()
            },
            Some("card"),
            config.clone(),
            local.clone(),
        )?;
        assert_eq!(state.game().save_group.as_deref(), Some("card"));
        assert!(state.installation().is_none());
        assert!(state.installations.installations.is_empty());
        state.game_config.validate_ownership(&state.installations)?;
        assert!(config.games.is_empty());
        let before = toml::to_string(&local)?;
        assert_eq!(toml::to_string(&state.installations)?, before);
        let _mock = crate::menu_utils::MockQueue::new()
            .cancel_selection()
            .confirm_yes()
            .guard();
        edit_menu::run_edit_menu("Discovered game", &mut state)?;
        assert_eq!(crate::menu_utils::scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn discovered_game_requires_a_save_choice_before_opening_the_editor() {
        assert!(
            prepare_prefilled_state(
                AddGameOptions {
                    name: Some("Game".into()),
                    ..Default::default()
                },
                None,
                InstantGameConfig::default(),
                InstallationsConfig::default()
            )
            .is_err()
        );
    }

    #[test]
    #[serial_test::serial]
    fn action_menu_preserves_direct_and_nested_navigation() -> Result<()> {
        use crate::dot::test_util::EnvGuard;
        use crate::menu_utils::MockQueue;
        let temp = tempfile::tempdir()?;
        let _config_home = EnvGuard::set("XDG_CONFIG_HOME", temp.path());
        let config = InstantGameConfig {
            games: vec![Game::new("Navigation test")],
            ..Default::default()
        };
        config.save()?;
        InstallationsConfig::default().save()?;
        for direct in [false, true] {
            let _mock = MockQueue::new().cancel_selection().guard();
            let result = run_game_action_menu("Navigation test", direct)?;
            assert!(matches!(
                (direct, result),
                (true, ActionResult::Exit) | (false, ActionResult::Back)
            ));
            assert_eq!(crate::menu_utils::scripted_responses_remaining(), 0);
        }
        // Setup, launch, edit, back: without a launch command or save binding,
        // these are the only actions. Selecting Back has the same semantics as Escape.
        for direct in [false, true] {
            let _mock = MockQueue::new().select_index(3).guard();
            let result = run_game_action_menu("Navigation test", direct)?;
            assert!(matches!(
                (direct, result),
                (true, ActionResult::Exit) | (false, ActionResult::Back)
            ));
            assert_eq!(crate::menu_utils::scripted_responses_remaining(), 0);
        }
        // Cancelling the launch-command builder stays in the action menu.
        let _mock = MockQueue::new()
            .select_index(1)
            .cancel_selection()
            .cancel_selection()
            .guard();
        assert!(matches!(
            run_game_action_menu("Navigation test", false)?,
            ActionResult::Back
        ));
        assert_eq!(crate::menu_utils::scripted_responses_remaining(), 0);
        Ok(())
    }
}
