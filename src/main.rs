use anyhow::Result;
use colored::*;

mod dot;

use clap::{Parser, Subcommand};

use crate::dot::config::{ConfigManager, Repo, extract_repo_name};
use crate::dot::db::Database;
use crate::dot::repo::cli::{RepoCommands, SubdirCommands};

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

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.debug {
        eprintln!("Debug mode is on");
    }

    // Load configuration once at startup
    let mut config_manager = match ConfigManager::load_from(cli.config.as_deref()) {
        Ok(manager) => manager,
        Err(e) => {
            eprintln!(
                "{}: {}",
                "Error loading configuration".red(),
                e.to_string().red()
            );
            return Err(e);
        }
    };

    // Ensure directories exist and create database instance once at startup
    config_manager.config.ensure_directories()?;
    let db = match Database::new(config_manager.config.database_path().to_path_buf()) {
        Ok(db) => db,
        Err(e) => {
            eprintln!(
                "{}: {}",
                "Error opening database".red(),
                e.to_string().red()
            );
            return Err(e);
        }
    };

    match &cli.command {
        Some(Commands::Dot { command }) => match command {
            DotCommands::Repo { command } => {
                match dot::repo::commands::handle_repo_command(&mut config_manager, &db, command, cli.debug) {
                    Ok(()) => (),
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error handling repository command".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
            DotCommands::Reset { path } => {
                if let Err(e) = dot::reset_modified(&config_manager.config, &db, &path) {
                    eprintln!(
                        "{}: {}",
                        "Error resetting dotfiles".red(),
                        e.to_string().red()
                    );
                    return Err(e);
                }
            }
            DotCommands::Apply => match dot::apply_all(&config_manager.config, &db) {
                Ok(()) => println!("{}", "Applied dotfiles".green()),
                Err(e) => {
                    eprintln!(
                        "{}: {}",
                        "Error applying dotfiles".red(),
                        e.to_string().red()
                    );
                    return Err(e);
                }
            },
            DotCommands::Fetch { path, dry_run } => {
                match dot::fetch_modified(&config_manager.config, &db, path.as_deref(), *dry_run) {
                    Ok(()) => println!("{}", "Fetched modified dotfiles".green()),
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error fetching dotfiles".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
            DotCommands::Add { path } => match dot::add_dotfile(&config_manager.config, &db, &path)
            {
                Ok(()) => println!("{} {}", "Added dotfile".green(), path.green()),
                Err(e) => {
                    eprintln!("{}: {}", "Error adding dotfile".red(), e.to_string().red());
                    return Err(e);
                }
            },
            DotCommands::Update => match dot::update_all(&config_manager.config, cli.debug) {
                Ok(()) => println!("{}", "All repos updated".green()),
                Err(e) => {
                    eprintln!("{}: {}", "Error updating repos".red(), e.to_string().red());
                    return Err(e);
                }
            },
            DotCommands::Status { path } => {
                match dot::status_all(&config_manager.config, cli.debug, path.as_deref(), &db) {
                    Ok(()) => (),
                    Err(e) => {
                        eprintln!("Error checking repo status: {}", e);
                        return Err(e);
                    }
                }
            }
            DotCommands::Init {
                name,
                non_interactive,
            } => {
                let cwd = std::env::current_dir().map_err(|e| anyhow::anyhow!("Unable to determine current directory: {}", e))?;
                match dot::meta::init_repo(&cwd, name.as_deref(), *non_interactive) {
                    Ok(()) => println!(
                        "{} {}",
                        "Initialized instantdots.toml in".green(),
                        cwd.display()
                    ),
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error initializing repo".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
        },
        None => {
            println!("instant: run with --help for usage");
        }
    }
    Ok(())
}
