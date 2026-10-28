mod edit_menu;
mod editors;
mod state;

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::TildePath;
use crate::game::GameCommands;
use crate::game::config::{Game, GameInstallation, PathContentKind};
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::AddGameOptions;
use crate::game::games::GameManager;
use crate::game::games::selection::{GameMenuEntry, select_game_menu_entry};
use crate::game::handle_game_command;
use crate::game::launch_command::LaunchCommand;
use crate::game::operations::desktop;
use crate::game::operations::launch_game;
use crate::game::operations::steam;
use crate::game::operations::sync::sync_game_saves;
use crate::game::platforms::LaunchCommandBuilderContext;
use crate::game::restic;
use crate::game::setup;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{
    ConfirmResult, FzfSelectable, FzfWrapper, Header, HeaderBuilder, MenuCursor,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use instantos_terminal::Launcher;

use state::EditState;

/// Game action selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameAction {
    Launch,
    Edit,
    OpenSaveDirectory,
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
            GameAction::OpenSaveDirectory => write!(f, "open-save-directory"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveDirectoryOpenMethod {
    Yazi,
    Terminal,
    XdgOpen,
    Back,
}

impl std::fmt::Display for SaveDirectoryOpenMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveDirectoryOpenMethod::Yazi => write!(f, "yazi"),
            SaveDirectoryOpenMethod::Terminal => write!(f, "terminal"),
            SaveDirectoryOpenMethod::XdgOpen => write!(f, "xdg-open"),
            SaveDirectoryOpenMethod::Back => write!(f, "back"),
        }
    }
}

#[derive(Clone)]
struct SaveDirectoryOpenItem {
    display: String,
    method: SaveDirectoryOpenMethod,
    preview: FzfPreview,
}

impl FzfSelectable for SaveDirectoryOpenItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.method.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
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

        let has_installation = game_config
            .resolve_owner(&installations, &game_config.owner_id(game_name))
            .is_ok();

        let game = game_config.games.iter().find(|g| g.name.0 == game_name);
        let game_cmd = game.and_then(|g| g.launch_command.clone());
        let description = game.and_then(|g| g.description.clone());

        let inst_cmd = installation.and_then(|i| i.launch_command.clone());

        let resolved = game_config
            .resolve_owner(&installations, &game_config.owner_id(game_name))
            .ok();
        let save_path = resolved.as_ref().map(|i| {
            i.save_path
                .to_tilde_string()
                .unwrap_or_else(|_| i.save_path.as_path().to_string_lossy().to_string())
        });

        // Installation command takes precedence over game command
        let launch_command = inst_cmd.or(game_cmd).map(|command| command.to_string());

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
    let mut actions: Vec<GameActionItem> = Vec::new();

    if state.needs_setup {
        actions.push(action_setup_item(game_name));
    }

    actions.push(action_launch_item(game_name, state));
    actions.push(action_edit_item(game_name, state));

    if !state.needs_setup {
        if let Some(path) = &state.save_path {
            actions.push(action_open_save_item(game_name, path));
            actions.push(action_move_item(game_name, path));
        }
        actions.push(action_checkpoint_item(game_name));
    }

    if state.launch_command.is_some() {
        actions.push(action_steam_item(game_name));
        actions.push(action_desktop_item(game_name));
    }

    actions.push(action_back_item());
    actions
}

fn action_setup_item(game_name: &str) -> GameActionItem {
    GameActionItem {
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
    }
}

fn action_launch_item(game_name: &str, state: &GameState) -> GameActionItem {
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

    GameActionItem {
        display: format!(
            "{} Launch",
            format_icon_colored(NerdFont::Rocket, colors::GREEN)
        ),
        action: GameAction::Launch,
        preview: builder.build(),
        keywords: vec!["play", "start", "run"],
    }
}

fn action_edit_item(game_name: &str, state: &GameState) -> GameActionItem {
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

    GameActionItem {
        display: format!(
            "{} Edit",
            format_icon_colored(NerdFont::Edit, colors::SAPPHIRE)
        ),
        action: GameAction::Edit,
        preview: builder.build(),
        keywords: vec!["change", "modify", "configure", "settings"],
    }
}

fn action_open_save_item(game_name: &str, path: &str) -> GameActionItem {
    GameActionItem {
        display: format!(
            "{} Open Save Directory",
            format_icon_colored(NerdFont::FolderOpen, colors::GREEN)
        ),
        action: GameAction::OpenSaveDirectory,
        preview: PreviewBuilder::new()
            .header(NerdFont::FolderOpen, "Open Save Directory")
            .text(&format!("Open the save location for '{}'.", game_name))
            .blank()
            .field("Configured path", path)
            .blank()
            .text("Choose how to open it:")
            .bullet("yazi file manager")
            .bullet("Terminal window in that directory")
            .bullet("Default desktop application via xdg-open")
            .build(),
        keywords: vec!["folder", "browse", "explore", "files"],
    }
}

fn action_move_item(game_name: &str, path: &str) -> GameActionItem {
    GameActionItem {
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
    }
}

fn action_checkpoint_item(game_name: &str) -> GameActionItem {
    GameActionItem {
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
    }
}

fn action_steam_item(game_name: &str) -> GameActionItem {
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

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Steam, "Add to Steam")
        .text(&format!("Manage Steam shortcut for '{}'.", game_name))
        .blank()
        .text(&status_text);

    if is_in_steam && let Ok(Some(shortcut)) = steam::get_game_shortcut(game_name) {
        builder = builder.blank().text("Current shortcut details:");
        builder = builder
            .subtext(&format!("Exe:        {}", shortcut.exe))
            .subtext(&format!("Start dir:  {}", shortcut.start_dir));

        if !shortcut.launch_options.is_empty() {
            builder = builder.subtext(&format!("Options:    {}", shortcut.launch_options));
        }

        let full_cmd = if shortcut.launch_options.is_empty() {
            shortcut.exe.clone()
        } else {
            format!("{} {}", shortcut.exe, shortcut.launch_options)
        };
        builder = builder
            .blank()
            .subtext(&format!("Full command: {}", full_cmd));
    }

    let preview = builder
        .blank()
        .text("The shortcut will launch the game through ins,")
        .text("which automatically syncs saves before and after.")
        .blank()
        .subtext("Note: Restart Steam after adding to see the shortcut.")
        .build();

    GameActionItem {
        display: format!(
            "{} Add to Steam",
            format_icon_colored(NerdFont::Steam, colors::SAPPHIRE)
        ),
        action: GameAction::AddToSteam,
        preview,
        keywords: vec![],
    }
}

fn action_desktop_item(game_name: &str) -> GameActionItem {
    let is_on_desktop = desktop::is_game_on_desktop(game_name).unwrap_or(false);
    let status_text = if is_on_desktop {
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

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Desktop, "Add to Desktop")
        .text(&format!("Manage desktop shortcut for '{}'.", game_name))
        .blank()
        .text(&status_text);

    if is_on_desktop && let Ok(Some(path)) = desktop::get_game_desktop_path(game_name) {
        builder = builder
            .blank()
            .text("Current shortcut location:")
            .subtext(&path.display().to_string());
    }

    let preview = builder
        .blank()
        .text("Creates a .desktop file that launches the game through ins,")
        .text("which automatically syncs saves before and after.")
        .blank()
        .subtext("The shortcut will be placed on your Desktop if possible,")
        .subtext("otherwise in the applications menu.")
        .build();

    GameActionItem {
        display: format!(
            "{} Add to Desktop",
            format_icon_colored(NerdFont::Desktop, colors::MAUVE)
        ),
        action: GameAction::AddToDesktop,
        preview,
        keywords: vec!["shortcut", "launcher"],
    }
}

fn action_back_item() -> GameActionItem {
    GameActionItem {
        display: format!("{} Back", format_back_icon()),
        action: GameAction::Back,
        preview: PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to game selection.")
            .build(),
        keywords: vec![],
    }
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
        GameAction::Launch => handle_launch_action(game_name, state),
        GameAction::Edit => {
            run_edit_menu_for_game(game_name, &state.game_config, &state.installations)?;
            Ok(if exit_after {
                ActionResult::Exit
            } else {
                ActionResult::Stay
            })
        }
        GameAction::OpenSaveDirectory => {
            handle_open_save_directory_action(game_name, state)?;
            Ok(ActionResult::Stay)
        }
        GameAction::Setup => {
            setup::setup_uninstalled_games()?;
            Ok(if exit_after {
                ActionResult::Exit
            } else {
                ActionResult::Stay
            })
        }
        GameAction::Move => {
            GameManager::relocate_game(Some(game_name.to_string()), None)?;
            Ok(if exit_after {
                ActionResult::Exit
            } else {
                ActionResult::Stay
            })
        }
        GameAction::Checkpoint => {
            handle_checkpoint_action(game_name)?;
            Ok(if exit_after {
                ActionResult::Exit
            } else {
                ActionResult::Stay
            })
        }
        GameAction::AddToSteam => {
            let Some(launch_cmd) = ensure_launch_command(game_name, state)? else {
                return Ok(ActionResult::Stay);
            };
            handle_steam_action(game_name, &launch_cmd)?;
            Ok(ActionResult::Stay)
        }
        GameAction::AddToDesktop => {
            let Some(launch_cmd) = ensure_launch_command(game_name, state)? else {
                return Ok(ActionResult::Stay);
            };
            handle_desktop_action(game_name, &launch_cmd)?;
            Ok(ActionResult::Stay)
        }
        GameAction::Back => Ok(if exit_after {
            ActionResult::Exit
        } else {
            ActionResult::Back
        }),
    }
}

fn handle_launch_action(game_name: &str, state: &GameState) -> Result<ActionResult> {
    if ensure_launch_command(game_name, state)?.is_none() {
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

fn ensure_launch_command(game_name: &str, state: &GameState) -> Result<Option<String>> {
    if let Some(command) = state.launch_command.as_deref() {
        return Ok(Some(command.to_string()));
    }

    let installation = state
        .installations
        .installations
        .iter()
        .find(|install| install.game_name.0 == game_name);
    let context = LaunchCommandBuilderContext::from_game(
        installation.and_then(|install| {
            install
                .saves
                .as_ref()
                .map(|saves| saves.save_path.as_path())
        }),
        installation
            .and_then(|install| install.launch_command.as_ref())
            .or_else(|| {
                state
                    .game_config
                    .games
                    .iter()
                    .find(|game| game.name.0 == game_name)
                    .and_then(|game| game.launch_command.as_ref())
            }),
    );

    let Some(command) = crate::game::platforms::build_launch_command_with_context(Some(&context))?
    else {
        return Ok(None);
    };

    let mut game_config = state.game_config.clone();
    let Some(game) = game_config.games.iter_mut().find(|g| g.name.0 == game_name) else {
        return Ok(None);
    };
    game.launch_command = Some(command.clone());
    game_config.save()?;
    FzfWrapper::message(&format!(
        "{} Launch command saved.",
        char::from(NerdFont::Check)
    ))?;

    Ok(Some(command.to_string()))
}

fn handle_steam_action(game_name: &str, launch_cmd: &str) -> Result<()> {
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
                Err(e) => FzfWrapper::message(&format!("Failed to remove from Steam: {}", e))?,
            }
        }
    } else {
        match steam::add_game_to_steam(game_name, launch_cmd) {
            Ok((true, steam_running)) => {
                let base_msg = format!("Added '{}' to Steam as a non-Steam game.", game_name);
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
            Err(e) => FzfWrapper::message(&format!("Failed to add to Steam: {}", e))?,
        }
    }
    Ok(())
}

fn handle_desktop_action(game_name: &str, launch_cmd: &str) -> Result<()> {
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
                Ok(true) => FzfWrapper::message(&format!("Removed '{}' from Desktop.", game_name))?,
                Ok(false) => FzfWrapper::message(&format!(
                    "'{}' was not found on the Desktop (maybe already removed).",
                    game_name
                ))?,
                Err(e) => FzfWrapper::message(&format!("Failed to remove from Desktop: {}", e))?,
            }
        }
    } else {
        match desktop::add_game_to_desktop(game_name, launch_cmd) {
            Ok((true, Some(path))) => FzfWrapper::message(&format!(
                "Added '{}' to Desktop.\n\nLocation: {}",
                game_name,
                path.display()
            ))?,
            Ok((true, None)) => {
                FzfWrapper::message(&format!("Added '{}' to Desktop.", game_name))?;
            }
            Ok((false, _)) => {
                FzfWrapper::message(&format!("'{}' is already on your Desktop.", game_name))?;
            }
            Err(e) => FzfWrapper::message(&format!("Failed to add to Desktop: {}", e))?,
        }
    }
    Ok(())
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

fn handle_open_save_directory_action(game_name: &str, state: &GameState) -> Result<()> {
    let owner = state
        .game_config
        .resolve_owner(&state.installations, &state.game_config.owner_id(game_name))?;
    let target_dir = save_directory_target(&owner.saves);
    let target_display = target_dir.display().to_string();

    let items = vec![
        SaveDirectoryOpenItem {
            display: format!(
                "{} Open in yazi",
                format_icon_colored(NerdFont::FolderOpen, colors::SAPPHIRE)
            ),
            method: SaveDirectoryOpenMethod::Yazi,
            preview: PreviewBuilder::new()
                .header(NerdFont::FolderOpen, "Open in yazi")
                .text("Open the save directory in the yazi file manager.")
                .blank()
                .field("Directory", &target_display)
                .build(),
        },
        SaveDirectoryOpenItem {
            display: format!(
                "{} Open terminal here",
                format_icon_colored(NerdFont::Terminal, colors::GREEN)
            ),
            method: SaveDirectoryOpenMethod::Terminal,
            preview: PreviewBuilder::new()
                .header(NerdFont::Terminal, "Open Terminal")
                .text("Open a terminal window in the save directory.")
                .blank()
                .field("Directory", &target_display)
                .build(),
        },
        SaveDirectoryOpenItem {
            display: format!(
                "{} Open with xdg-open",
                format_icon_colored(NerdFont::Folder, colors::YELLOW)
            ),
            method: SaveDirectoryOpenMethod::XdgOpen,
            preview: PreviewBuilder::new()
                .header(NerdFont::Folder, "Open with xdg-open")
                .text("Open the save directory with the desktop default application.")
                .blank()
                .field("Directory", &target_display)
                .build(),
        },
        SaveDirectoryOpenItem {
            display: format!("{} Back", format_back_icon()),
            method: SaveDirectoryOpenMethod::Back,
            preview: PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to the game action menu.")
                .build(),
        },
    ];

    let selection = FzfWrapper::builder()
        .header(
            HeaderBuilder::new(NerdFont::FolderOpen, "Open Save Directory")
                .field("Game", game_name)
                .build(),
        )
        .prompt("Open with")
        .responsive_layout()
        .items(items)
        .padded()
        .select_one()?;

    let method = match selection {
        crate::menu_utils::DialogOutcome::Submitted(item) => item.method,
        crate::menu_utils::DialogOutcome::Cancelled => SaveDirectoryOpenMethod::Back,
    };

    match method {
        SaveDirectoryOpenMethod::Yazi => open_directory_in_yazi(&target_dir),
        SaveDirectoryOpenMethod::Terminal => open_directory_in_terminal(&target_dir),
        SaveDirectoryOpenMethod::XdgOpen => open_directory_with_xdg_open(&target_dir),
        SaveDirectoryOpenMethod::Back => Ok(()),
    }
}

fn save_directory_target(saves: &crate::game::config::OwnedSaves) -> PathBuf {
    let save_path = saves.save_path.as_path();
    match saves.save_path_type {
        PathContentKind::File => save_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| save_path.to_path_buf()),
        _ => save_path.to_path_buf(),
    }
}

fn open_directory_in_yazi(path: &Path) -> Result<()> {
    Launcher::new("yazi")
        .arg(path.as_os_str())
        .title("Save Directory")
        .spawn_detached()
        .map(|_| ())
        .map_err(Into::into)
}

fn open_directory_in_terminal(path: &Path) -> Result<()> {
    Launcher::default()
        .working_directory(path)
        .title("Save Directory")
        .spawn_detached()
        .map(|_| ())
        .map_err(Into::into)
}

fn open_directory_with_xdg_open(path: &Path) -> Result<()> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .context("Failed to open save directory with xdg-open")?;
    Ok(())
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
                .header(HeaderBuilder::new(NerdFont::Gamepad, name).build())
                .prompt("Select action")
                .responsive_layout();

            if let Some(index) = cursor.initial_index(&actions) {
                builder = builder.initial_index(index);
            }

            let selection = builder.items(actions.clone()).padded().select_one()?;

            let result = match selection {
                crate::menu_utils::DialogOutcome::Submitted(item) => {
                    cursor.update(&item, &actions);
                    handle_action(item.action, name, &state, exit_after_action)?
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
                handle_game_command(
                    GameCommands::Add {
                        save_group: None,
                        name: None,
                        description: None,
                        launch_command: None,
                        save_path: None,
                        create_save_path: false,
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
                // Inner loop: game action menu
                let mut cursor = MenuCursor::new();
                loop {
                    let state = GameState::load(&game_name)?;
                    let actions = build_action_menu(&game_name, &state);

                    let mut builder = FzfWrapper::builder()
                        .header(HeaderBuilder::new(NerdFont::Gamepad, &game_name).build())
                        .prompt("Select action")
                        .responsive_layout();

                    if let Some(index) = cursor.initial_index(&actions) {
                        builder = builder.initial_index(index);
                    }

                    let selection = builder.items(actions.clone()).padded().select_one()?;

                    let result = match selection {
                        crate::menu_utils::DialogOutcome::Submitted(item) => {
                            cursor.update(&item, &actions);
                            handle_action(item.action, &game_name, &state, false)?
                        }
                        crate::menu_utils::DialogOutcome::Cancelled => ActionResult::Back,
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
    fn save_directory_target_uses_parent_for_file_paths() {
        let installation = GameInstallation::with_kind(
            "Test Game",
            TildePath::new(PathBuf::from("/tmp/game/saves/slot1.sav")),
            PathContentKind::File,
        );

        assert_eq!(
            save_directory_target(installation.owned().unwrap()),
            PathBuf::from("/tmp/game/saves")
        );
    }

    #[test]
    fn save_directory_target_keeps_directory_paths() {
        let installation = GameInstallation::with_kind(
            "Test Game",
            TildePath::new(PathBuf::from("/tmp/game/saves")),
            PathContentKind::Directory,
        );

        assert_eq!(
            save_directory_target(installation.owned().unwrap()),
            PathBuf::from("/tmp/game/saves")
        );
    }
}
