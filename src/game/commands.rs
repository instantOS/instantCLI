use anyhow::Result;

use super::cli::GameCommands;
use super::games::GameManager;
use super::games::{display, selection};
use super::operations::{launch_game, sync_game_saves};
use super::repository::RepositoryManager;
use super::restic::{backup_game_saves, handle_restic_command, restore_game_saves};
use super::setup;

#[cfg(debug_assertions)]
use super::cli::DebugCommands;

pub fn handle_game_command(command: GameCommands, debug: bool) -> Result<()> {
    match command {
        GameCommands::Init => handle_init(debug),
        GameCommands::Add => handle_add(),
        GameCommands::Sync { game_name, force } => handle_sync(game_name, force),
        GameCommands::Launch { game_name } => handle_launch(game_name),
        GameCommands::List => handle_list(),
        GameCommands::Show { game_name } => handle_show(game_name),
        GameCommands::Remove { game_name } => handle_remove(game_name),
        GameCommands::Backup { game_name } => handle_backup(game_name),
        GameCommands::Restic { args } => handle_restic_command(args),
        GameCommands::Restore {
            game_name,
            snapshot_id,
            force,
        } => handle_restore(game_name, snapshot_id, force),
        GameCommands::Setup => handle_setup(),
        #[cfg(debug_assertions)]
        GameCommands::Debug { debug_command } => handle_debug(debug_command),
    }
}

fn handle_init(debug: bool) -> Result<()> {
    RepositoryManager::initialize_game_manager(debug)
}

fn handle_add() -> Result<()> {
    GameManager::add_game()
}

fn handle_sync(game_name: Option<String>, force: bool) -> Result<()> {
    sync_game_saves(game_name, force)
}

fn handle_launch(game_name: String) -> Result<()> {
    launch_game(game_name)
}

fn handle_remove(game_name: Option<String>) -> Result<()> {
    GameManager::remove_game(game_name)
}

fn handle_list() -> Result<()> {
    display::list_games()
}

fn handle_show(game_name: Option<String>) -> Result<()> {
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

    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    
    let restic = ResticWrapper::new(
        game_config.repo.as_path().to_string_lossy().to_string(),
        game_config.repo_password.clone(),
    );

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
    print!("{}", debug_output);

    Ok(())
}
