use anyhow::Result;
use colored::*;

mod common;
mod completions;
mod dev;
mod doctor;
mod dot;
mod game;
mod launch;
mod menu;
mod menu_utils;
mod restic;
mod scratchpad;
mod settings;
mod ui;
mod video;

use clap::{CommandFactory, Parser, Subcommand, ValueHint};

/// Helper function to format and print errors consistently
fn handle_error(context: &str, error: &anyhow::Error) -> String {
    use std::fmt::Write as _;
    // Print the top-level error and then the full cause chain for better diagnostics
    let mut msg = format!("{}: {}", context.red(), error.to_string().red());
    for cause in error.chain().skip(1) {
        let _ = write!(&mut msg, "\n  Caused by: {}", cause);
    }
    msg
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
use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::cli::RepoCommands;
use crate::scratchpad::ScratchpadCommand;
use crate::settings::SettingsCommands;
use crate::ui::prelude::*;

/// InstantCLI main parser
#[derive(clap::ValueEnum, Clone, Debug)]
enum OutputFormatArg {
    Text,
    Json,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Activate debug mode
    #[arg(short, long, global = true)]
    debug: bool,

    /// Custom config file path
    #[arg(short = 'c', long = "config", global = true)]
    config: Option<String>,

    /// Output format for machine-readable integration
    #[arg(long = "output", value_enum, default_value_t = OutputFormatArg::Text, global = true)]
    output: OutputFormatArg,

    /// Disable colored output
    #[arg(long = "no-color", global = true)]
    no_color: bool,

    /// Force menu fallback mode using transient kitty terminals
    #[arg(long = "menu-fallback", global = true)]
    menu_fallback: bool,

    /// Internal flag set when restarted with sudo
    #[arg(long, hide = true)]
    internal_privileged_mode: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

pub(crate) fn cli_command() -> clap::Command {
    Cli::command()
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Dotfile management commands
    Dot {
        #[command(subcommand)]
        command: DotCommands,
    },
    /// Game save management commands
    Game {
        #[command(subcommand)]
        command: game::GameCommands,
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
    /// Application launcher
    Launch {
        /// List available applications instead of launching
        #[arg(long)]
        list: bool,
    },
    /// Interactive menu commands for shell scripts
    Menu {
        #[command(subcommand)]
        command: menu::MenuCommands,
    },
    /// Scratchpad terminal management
    Scratchpad {
        #[command(subcommand)]
        command: ScratchpadCommand,
    },
    /// Desktop settings and preferences
    Settings {
        #[command(subcommand)]
        command: Option<SettingsCommands>,
        /// Navigate directly to a specific setting by ID (e.g., "appearance.animations")
        #[arg(short = 's', long = "setting", conflicts_with_all = ["category", "search"])]
        setting: Option<String>,
        /// Navigate directly to a specific category (e.g., "appearance", "desktop")
        #[arg(long = "category", conflicts_with_all = ["setting", "search"])]
        category: Option<String>,
        /// Start in search mode to browse all settings
        #[arg(long = "search", conflicts_with_all = ["setting", "category"])]
        search: bool,
    },
    /// Video transcription and editing utilities
    Video {
        #[command(subcommand)]
        command: video::VideoCommands,
    },
    /// Debugging and diagnostic utilities
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
    /// Shell completion helpers
    Completions {
        #[command(subcommand)]
        command: completions::CompletionCommands,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum DebugCommands {
    /// View restic command logs
    ResticLogs {
        /// Number of recent logs to show (default: 10)
        #[arg(short, long)]
        limit: Option<usize>,
        /// Clear all logs
        #[arg(long)]
        clear: bool,
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
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
    },
    /// Apply dotfiles
    Apply,
    /// Add or update dotfiles
    ///
    /// For a single file: If tracked, update the source file. If untracked, prompt to add it.
    /// For a directory: Update all tracked files. Use --all to also add untracked files.
    Add {
        /// Path to add or update (relative to ~)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
        /// Recursively add all files in directory, including untracked ones
        #[arg(long)]
        all: bool,
    },
    /// Pull updates for all configured repos and apply changes
    Update,
    /// Check dotfile status
    Status {
        /// Optional path to a dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        #[arg(value_hint = ValueHint::AnyPath)]
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
    /// Show differences between modified dotfiles and their source
    Diff {
        /// Optional path to a dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: Option<String>,
    },
}

fn handle_debug_command(command: DebugCommands) -> Result<()> {
    use crate::restic::logging::ResticCommandLogger;

    match command {
        DebugCommands::ResticLogs { limit, clear } => {
            let logger = ResticCommandLogger::new()?;

            if clear {
                logger.clear_logs()?;
                emit(
                    Level::Success,
                    "restic.logs.cleared",
                    &format!(
                        "{} Cleared all restic command logs.",
                        char::from(NerdFont::Trash)
                    ),
                    None,
                );
            } else {
                logger.print_recent_logs(limit)?;
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    clap_complete::CompleteEnv::with_factory(cli_command).complete();

    let cli = Cli::parse();

    // Initialize UI renderer
    let format = match cli.output {
        OutputFormatArg::Text => ui::OutputFormat::Text,
        OutputFormatArg::Json => ui::OutputFormat::Json,
    };
    ui::init(format, !cli.no_color);

    if cli.menu_fallback {
        menu::client::force_fallback_mode();
    }

    if cli.debug {
        eprintln!("Debug mode is on");
        // Set global debug mode for restic logging
        crate::restic::logging::set_debug_mode(true);
    }

    match &cli.command {
        Some(Commands::Game { command }) => {
            execute_with_error_handling(
                game::handle_game_command(command.clone(), cli.debug),
                "Error handling game command",
                None,
            )?;
        }
        Some(Commands::Dot { command }) => {
            // Load configuration once at startup
            let mut config = execute_with_error_handling(
                Config::load(cli.config.as_deref()),
                "Error loading configuration",
                None,
            )?;

            // Ensure directories exist and create database instance once at startup
            config.ensure_directories()?;
            let db = execute_with_error_handling(
                Database::new(config.database_path().to_path_buf()),
                "Error opening database",
                None,
            )?;

            match command {
                DotCommands::Repo { command } => {
                    execute_with_error_handling(
                        dot::repo::commands::handle_repo_command(
                            &mut config,
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
                        dot::reset_modified(&config, &db, path),
                        "Error resetting dotfiles",
                        None,
                    )?;
                }
                DotCommands::Apply => {
                    execute_with_error_handling(
                        dot::apply_all(&config, &db),
                        "Error applying dotfiles",
                        Some("Applied dotfiles"),
                    )?;
                }
                DotCommands::Add { path, all } => {
                    execute_with_error_handling(
                        dot::add_dotfile(&config, &db, path, *all),
                        "Error adding/updating dotfile",
                        None, // Success messages are handled within add_dotfile
                    )?;
                }
                DotCommands::Update => {
                    execute_with_error_handling(
                        dot::update_all(&config, cli.debug),
                        "Error updating repos",
                        Some("All repos updated"),
                    )?;
                }
                DotCommands::Status { path, all } => {
                    execute_with_error_handling(
                        dot::status_all(&config, cli.debug, path.as_deref(), &db, *all),
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
                DotCommands::Diff { path } => {
                    execute_with_error_handling(
                        dot::diff_all(&config, cli.debug, path.as_deref(), &db),
                        "Error showing dotfile differences",
                        None,
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
        Some(Commands::Launch { list }) => {
            let exit_code = launch::handle_launch_command(*list).await?;
            std::process::exit(exit_code);
        }
        Some(Commands::Doctor { command }) => {
            doctor::command::handle_doctor_command(command.clone()).await?;
        }
        Some(Commands::Menu { command }) => {
            let exit_code = menu::handle_menu_command(command.clone(), cli.debug).await?;
            std::process::exit(exit_code);
        }
        Some(Commands::Scratchpad { command }) => {
            let compositor = common::compositor::CompositorType::detect();
            let exit_code = command.clone().run(&compositor, cli.debug)?;
            std::process::exit(exit_code);
        }
        Some(Commands::Settings {
            command,
            setting,
            category,
            search,
        }) => {
            use settings::SettingsNavigation;
            let navigation = if let Some(setting_id) = setting {
                Some(SettingsNavigation::Setting(setting_id.clone()))
            } else if let Some(category_id) = category {
                Some(SettingsNavigation::Category(category_id.clone()))
            } else if *search {
                Some(SettingsNavigation::Search)
            } else {
                None
            };

            execute_with_error_handling(
                settings::dispatch_settings_command(
                    cli.debug,
                    cli.internal_privileged_mode,
                    command.clone(),
                    navigation,
                ),
                "Error running settings",
                None,
            )?;
        }
        Some(Commands::Video { command }) => {
            execute_with_error_handling(
                video::handle_video_command(command.clone(), cli.debug),
                "Error handling video command",
                None,
            )?;
        }
        Some(Commands::Debug { command }) => {
            execute_with_error_handling(
                handle_debug_command(command.clone()),
                "Error handling debug command",
                None,
            )?;
        }
        Some(Commands::Completions { command }) => match command {
            completions::CompletionCommands::Generate { shell } => {
                let script = completions::generate(*shell)?;
                print!("{script}");
            }
            completions::CompletionCommands::Install {
                shell,
                snippet_only,
            } => {
                let instructions = completions::install(*shell, *snippet_only)?;
                println!("{instructions}");
            }
        },
        None => {
            emit(
                Level::Info,
                "cli.help",
                &format!("ℹ️ {}: run with --help for usage", env!("CARGO_BIN_NAME")),
                None,
            );
        }
    }
    Ok(())
}
