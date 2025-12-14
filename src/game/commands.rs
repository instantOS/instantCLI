use std::ffi::OsString;

use anyhow::Result;

use crate::common::deps::RESTIC;
use crate::common::package::{InstallResult, ensure_all};

use super::cli::{DependencyCommands, GameCommands};
use super::deps::{
    AddDependencyOptions, InstallDependencyOptions, UninstallDependencyOptions, add_dependency,
    install_dependency, list_dependencies as list_game_dependencies, uninstall_dependency,
};
use super::edit;
use super::games::GameManager;
use super::games::manager::AddGameOptions;
use super::games::{display, selection};
use super::operations::{exec_game_command, launch_game, sync_game_saves};
use super::repository::RepositoryManager;
use super::repository::manager::InitOptions;
use super::restic::{
    backup_game_saves, handle_restic_command, prune_snapshots, restore_game_saves,
};
use super::setup;

#[cfg(debug_assertions)]
use super::cli::DebugCommands;

/// Ensure restic is available, prompting for installation if needed
fn ensure_restic_available() -> Result<()> {
    match ensure_all(&[&RESTIC])? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(()),
        InstallResult::Declined => Err(anyhow::anyhow!("restic installation cancelled")),
        InstallResult::NotAvailable { hint, .. } => {
            Err(anyhow::anyhow!("restic not available: {}", hint))
        }
        InstallResult::Failed { reason } => {
            Err(anyhow::anyhow!("restic installation failed: {}", reason))
        }
    }
}

pub fn handle_game_command(command: GameCommands, debug: bool) -> Result<()> {
    match command {
        GameCommands::Init { repo, password } => {
            ensure_restic_available()?;
            handle_init(debug, repo, password)
        }
        GameCommands::Add {
            name,
            description,
            launch_command,
            save_path,
            create_save_path,
        } => handle_add(AddGameOptions {
            name,
            description,
            launch_command,
            save_path,
            create_save_path,
        }),
        GameCommands::Sync { game_name, force } => {
            ensure_restic_available()?;
            handle_sync(game_name, force)
        }
        GameCommands::Launch { game_name } => handle_launch(game_name),
        GameCommands::Exec { command } => handle_exec(command),
        GameCommands::List => handle_list(),
        GameCommands::Info { game_name } => handle_info(game_name),
        GameCommands::Edit { game_name } => edit::edit_game(game_name),
        GameCommands::Remove { game_name, force } => handle_remove(game_name, force),
        GameCommands::Backup { game_name } => {
            ensure_restic_available()?;
            handle_backup(game_name)
        }
        GameCommands::Prune {
            game_name,
            zero_changes,
        } => {
            ensure_restic_available()?;
            handle_prune(game_name, zero_changes)
        }
        GameCommands::Restic { args } => {
            ensure_restic_available()?;
            handle_restic_command(args)
        }
        GameCommands::Restore {
            game_name,
            snapshot_id,
            force,
        } => {
            ensure_restic_available()?;
            handle_restore(game_name, snapshot_id, force)
        }
        GameCommands::Setup => {
            ensure_restic_available()?;
            handle_setup()
        }
        GameCommands::Deps { command } => handle_dependency_command(command),
        #[cfg(debug_assertions)]
        GameCommands::Debug { debug_command } => handle_debug(debug_command),
    }
}

fn handle_init(debug: bool, repo: Option<String>, password: Option<String>) -> Result<()> {
    RepositoryManager::initialize_game_manager(debug, InitOptions { repo, password })
}

fn handle_add(options: AddGameOptions) -> Result<()> {
    GameManager::add_game(options)
}

fn handle_sync(game_name: Option<String>, force: bool) -> Result<()> {
    sync_game_saves(game_name, force)
}

fn handle_launch(game_name: Option<String>) -> Result<()> {
    launch_game(game_name)
}

fn handle_exec(command: Vec<OsString>) -> Result<()> {
    exec_game_command(command)
}

fn handle_remove(game_name: Option<String>, force: bool) -> Result<()> {
    GameManager::remove_game(game_name, force)
}

fn handle_list() -> Result<()> {
    display::list_games()
}

fn handle_info(game_name: Option<String>) -> Result<()> {
    let game_name = match game_name {
        Some(name) => name,
        None => match selection::select_game_interactive(None)? {
            Some(name) => name,
            None => return Ok(()),
        },
    };

    display::show_game_details(&game_name)
}

fn handle_backup(game_name: Option<String>) -> Result<()> {
    backup_game_saves(game_name)
}

fn handle_prune(game_name: Option<String>, zero_changes: bool) -> Result<()> {
    prune_snapshots(game_name, zero_changes)
}

fn handle_restore(
    game_name: Option<String>,
    snapshot_id: Option<String>,
    force: bool,
) -> Result<()> {
    restore_game_saves(game_name, snapshot_id, force)
}

fn handle_setup() -> Result<()> {
    setup::setup_uninstalled_games()
}

fn handle_dependency_command(command: DependencyCommands) -> Result<()> {
    match command {
        DependencyCommands::Add {
            game_name,
            dependency_id,
            path,
        } => add_dependency(AddDependencyOptions {
            game_name,
            dependency_id,
            source_path: path,
        }),
        DependencyCommands::Install {
            game_name,
            dependency_id,
            path,
        } => install_dependency(InstallDependencyOptions {
            game_name,
            dependency_id,
            install_path: path,
        }),
        DependencyCommands::Uninstall {
            game_name,
            dependency_id,
        } => uninstall_dependency(UninstallDependencyOptions {
            game_name,
            dependency_id,
        }),
        DependencyCommands::List { game_name } => list_game_dependencies(game_name),
    }
}

#[cfg(debug_assertions)]
fn handle_debug(debug_command: DebugCommands) -> Result<()> {
    match debug_command {
        DebugCommands::Tags { game_name } => handle_debug_tags(game_name),
    }
}

#[cfg(debug_assertions)]
fn handle_debug_tags(game_name: Option<String>) -> Result<()> {
    use crate::game::config::InstantGameConfig;
    use crate::game::restic::tags;
    use crate::restic::wrapper::ResticWrapper;
    use anyhow::Context;

    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    let restic = ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    )
    .context("Failed to initialize restic wrapper")?;

    let snapshots_json = if let Some(game_name) = game_name {
        // Get snapshots for specific game
        restic
            .list_snapshots_filtered(Some(tags::create_game_tags(&game_name)))
            .context("Failed to list snapshots for game")?
    } else {
        // Get all snapshots with instantgame tag
        restic
            .list_snapshots_filtered(Some(vec![tags::INSTANT_GAME_TAG.to_string()]))
            .context("Failed to list snapshots")?
    };

    let snapshots: Vec<crate::restic::wrapper::Snapshot> =
        serde_json::from_str(&snapshots_json).context("Failed to parse snapshot data")?;

    if snapshots.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }

    let debug_output = tags::debug_snapshot_tags(&snapshots);
    print!("{debug_output}");

    Ok(())
}
