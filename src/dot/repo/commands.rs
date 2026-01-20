mod apply;
mod clone;
mod info;
mod interactive;
mod list;
mod remove;
mod status;
mod subdirs;
mod toggle;

use super::cli::RepoCommands;
use crate::dot::config::Config;
use crate::dot::db::Database;
use anyhow::Result;

pub use clone::clone_repository;
pub use toggle::set_read_only_status;

/// Handle repository subcommands
pub fn handle_repo_command(
    config: &mut Config,
    db: &Database,
    command: &RepoCommands,
    debug: bool,
) -> Result<()> {
    match command {
        RepoCommands::List => list::list_repositories(config, db),
        RepoCommands::Clone(args) => clone::clone_repository(
            config,
            db,
            &args.url,
            args.name.as_deref(),
            args.branch.as_deref(),
            args.read_only,
            args.force_write,
            debug,
        ),
        RepoCommands::Remove { name, keep_files } => {
            remove::remove_repository(config, db, name, !*keep_files)
        }
        RepoCommands::Info { name } => info::show_repository_info(config, db, name),
        RepoCommands::Enable { name } => toggle::enable_repository(config, name),
        RepoCommands::Disable { name } => toggle::disable_repository(config, name),
        RepoCommands::Subdirs { command } => subdirs::handle_subdir_command(config, db, command),
        RepoCommands::SetReadOnly { name, read_only } => {
            toggle::set_read_only_status(config, name, *read_only)
        }
        RepoCommands::Status { name } => {
            status::show_repository_status(config, db, name.as_deref())
        }
        RepoCommands::Lazygit { name } => {
            interactive::open_repo_lazygit(config, db, name.as_deref())
        }
        RepoCommands::Shell { name } => interactive::open_repo_shell(config, db, name.as_deref()),
    }
}
