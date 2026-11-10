use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use instantos_terminal::Launcher;

use crate::game::config::{InstallationsConfig, PathContentKind};
use crate::game::games::GameManager;
use crate::game::operations::{desktop, launch_game, steam};
use crate::game::platforms::LaunchCommandBuilderContext;
use crate::game::{restic, setup};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{ConfirmResult, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::action_menu::GameAction;
use super::run_edit_menu_for_game;
use super::state::GameState;

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

/// Result of handling a game action
pub(super) enum ActionResult {
    Stay, // Stay in this game's action menu
    Back, // Go back to game selection
    Exit, // Exit the menu entirely
}

/// Handle a selected game action
pub(super) fn handle_action(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::game::config::GameInstallation;

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
