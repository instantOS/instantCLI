use anyhow::Result;
use colored::*;

mod common;
mod compositor;
mod dev;
mod doctor;
mod dot;
mod fzf_wrapper;
mod hyprland_ipc;
mod menu;
mod scratchpad;

use clap::{Parser, Subcommand};

/// Helper function to format and print errors consistently
fn handle_error(context: &str, error: &anyhow::Error) -> String {
    format!("{}: {}", context.red(), error.to_string().red())
}

/// Helper function to execute a fallible operation with consistent error handling
fn execute_with_error_handling<T>(
    operation: Result<T>,
    error_context: &str,
    success_message: Option<&str>,
) -> Result<T> {
    match operation {
        Ok(result) => {
            if let Some(msg) = success_message {
                println!("{}", msg.green());
            }
            Ok(result)
        }
        Err(e) => {
            eprintln!("{}", handle_error(error_context, &e));
            Err(e)
        }
    }
}

use crate::dev::DevCommands;
use crate::doctor::DoctorCommands;
use crate::dot::config::ConfigManager;
use crate::dot::db::Database;
use crate::dot::repo::cli::RepoCommands;
use crate::scratchpad::ScratchpadCommands;

/// InstantCLI main parser
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Activate debug mode
    #[arg(short, long, global = true)]
    debug: bool,

    /// Custom config file path
    #[arg(short = 'c', long = "config", global = true)]
    config: Option<String>,

    /// Internal flag set when restarted with sudo
    #[arg(long, hide = true)]
    internal_privileged_mode: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Dotfile management commands
    Dot {
        #[command(subcommand)]
        command: DotCommands,
    },
    /// System diagnostics and fixes
    Doctor {
        #[command(subcommand)]
        command: Option<DoctorCommands>,
    },
    /// Development utilities
    Dev {
        #[command(subcommand)]
        command: DevCommands,
    },
    /// Interactive menu commands for shell scripts
    Menu {
        #[command(subcommand)]
        command: menu::MenuCommands,
    },
    /// Scratchpad terminal management
    Scratchpad {
        #[command(subcommand)]
        command: ScratchpadCommands,
    },
}

#[derive(Subcommand, Debug)]
enum DotCommands {
    /// Repository management commands
    Repo {
        #[command(subcommand)]
        command: RepoCommands,
    },
    /// Reset modified dotfiles to their original state in the given path
    Reset {
        /// Path to reset (relative to ~)
        path: String,
    },
    /// Apply dotfiles
    Apply,
    /// Fetch modified dotfiles from home directory back to repository
    Fetch {
        /// Path to fetch (relative to ~)
        path: Option<String>,
        /// Perform a dry run, showing which files would be fetched
        #[arg(long)]
        dry_run: bool,
    },
    /// Add new dotfiles to tracking
    Add {
        /// Path to add (relative to ~)
        path: String,
    },
    /// Pull updates for all configured repos and apply changes
    Update,
    /// Check dotfile status
    Status {
        /// Optional path to a dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        path: Option<String>,
        /// Show all dotfiles including clean ones
        #[arg(long)]
        all: bool,
    },
    /// Initialize the repo in the current directory as an instantdots repo
    Init {
        /// Optional name to set in instantdots.toml
        name: Option<String>,
        /// Run non-interactively (use provided name or directory name)
        #[arg(long)]
        non_interactive: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.debug {
        eprintln!("Debug mode is on");
    }

    match &cli.command {
        Some(Commands::Dot { command }) => {
            // Load configuration once at startup
            let mut config_manager = execute_with_error_handling(
                ConfigManager::load_from(cli.config.as_deref()),
                "Error loading configuration",
                None,
            )?;

            // Ensure directories exist and create database instance once at startup
            config_manager.config.ensure_directories()?;
            let db = execute_with_error_handling(
                Database::new(config_manager.config.database_path().to_path_buf()),
                "Error opening database",
                None,
            )?;

            match command {
                DotCommands::Repo { command } => {
                    execute_with_error_handling(
                        dot::repo::commands::handle_repo_command(
                            &mut config_manager,
                            &db,
                            command,
                            cli.debug,
                        ),
                        "Error handling repository command",
                        None,
                    )?;
                }
                DotCommands::Reset { path } => {
                    execute_with_error_handling(
                        dot::reset_modified(&config_manager.config, &db, path),
                        "Error resetting dotfiles",
                        None,
                    )?;
                }
                DotCommands::Apply => {
                    execute_with_error_handling(
                        dot::apply_all(&config_manager.config, &db),
                        "Error applying dotfiles",
                        Some("Applied dotfiles"),
                    )?;
                }
                DotCommands::Fetch { path, dry_run } => {
                    execute_with_error_handling(
                        dot::fetch_modified(&config_manager.config, &db, path.as_deref(), *dry_run),
                        "Error fetching dotfiles",
                        Some("Fetched modified dotfiles"),
                    )?;
                }
                DotCommands::Add { path } => {
                    execute_with_error_handling(
                        dot::add_dotfile(&config_manager.config, &db, path),
                        "Error adding dotfile",
                        Some(&format!("Added dotfile {}", path.green())),
                    )?;
                }
                DotCommands::Update => {
                    execute_with_error_handling(
                        dot::update_all(&config_manager.config, cli.debug),
                        "Error updating repos",
                        Some("All repos updated"),
                    )?;
                }
                DotCommands::Status { path, all } => {
                    execute_with_error_handling(
                        dot::status_all(
                            &config_manager.config,
                            cli.debug,
                            path.as_deref(),
                            &db,
                            *all,
                        ),
                        "Error checking repo status",
                        None,
                    )?;
                }
                DotCommands::Init {
                    name,
                    non_interactive,
                } => {
                    let cwd = std::env::current_dir().map_err(|e| {
                        anyhow::anyhow!("Unable to determine current directory: {}", e)
                    })?;
                    execute_with_error_handling(
                        dot::meta::init_repo(&cwd, name.as_deref(), *non_interactive),
                        "Error initializing repo",
                        Some(&format!(
                            "Initialized instantdots.toml in {}",
                            cwd.display()
                        )),
                    )?;
                }
            }
        }
        Some(Commands::Dev { command }) => {
            execute_with_error_handling(
                dev::handle_dev_command(command.clone(), cli.debug).await,
                "Error handling dev command",
                None,
            )?;
        }
        Some(Commands::Doctor { command }) => {
            doctor::command::handle_doctor_command(command.clone()).await?;
        }
        Some(Commands::Menu { command }) => {
            let exit_code = menu::handle_menu_command(command.clone(), cli.debug).await?;
            std::process::exit(exit_code);
        }
        Some(Commands::Scratchpad { command }) => {
            execute_with_error_handling(
                scratchpad::handle_scratchpad_command(command.clone(), cli.debug),
                "Error handling scratchpad command",
                None,
            )?;
        }
        None => {
            println!("instant: run with --help for usage");
        }
    }
    Ok(())
}
