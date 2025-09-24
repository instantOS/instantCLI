use anyhow::Result;

use super::cli::GameCommands;
use super::repository::RepositoryManager;
use super::games::GameManager;
use super::games::{display, selection};
use super::operations::{sync_game_saves, launch_game};
use super::restic::{backup_game_saves, handle_restic_command};

pub fn handle_game_command(command: GameCommands, debug: bool) -> Result<()> {
    match command {
        GameCommands::Init => handle_init(debug),
        GameCommands::Add => handle_add(),
        GameCommands::Sync { game_name } => handle_sync(game_name),
        GameCommands::Launch { game_name } => handle_launch(game_name),
        GameCommands::List => handle_list(),
        GameCommands::Show { game_name } => handle_show(game_name),
        GameCommands::Remove { game_name } => handle_remove(game_name),
        GameCommands::Backup { game_name } => handle_backup(game_name),
        GameCommands::Restic { args } => handle_restic_command(args),
    }
}

fn handle_init(debug: bool) -> Result<()> {
    RepositoryManager::initialize_game_manager(debug)
}

fn handle_add() -> Result<()> {
    GameManager::add_game()
}

fn handle_sync(game_name: Option<String>) -> Result<()> {
    sync_game_saves(game_name)
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
        None => {
            match selection::select_game_interactive(None)? {
                Some(name) => name,
                None => return Ok(()),
            }
        }
    };

    display::show_game_details(&game_name)
}

fn handle_backup(game_name: Option<String>) -> Result<()> {
    backup_game_saves(game_name)
}
