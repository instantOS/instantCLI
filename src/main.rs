use anyhow::Result;
use colored::*;

mod arch;
mod assist;
mod autostart;
mod common;
mod completions;
mod debug;
mod dev;
mod doctor;
mod dot;
mod game;
mod launch;
mod menu;
mod menu_utils;
mod restic;
mod scratchpad;
mod self_update;
mod settings;
mod ui;
mod update;
mod video;
mod wallpaper;

use clap::{CommandFactory, Parser, Subcommand};

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

use crate::debug::DebugCommands;
use crate::dev::DevCommands;
use crate::doctor::DoctorCommands;
use crate::dot::commands::DotCommands;
use crate::scratchpad::ScratchpadCommand;
use crate::settings::SettingsCommands;

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
    /// Arch Linux installation commands
    Arch {
        #[command(subcommand)]
        command: arch::cli::ArchCommands,
    },
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
    /// Quick assist actions
    Assist {
        #[command(subcommand)]
        command: Option<assist::AssistCommands>,
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
    /// Wallpaper management commands
    Wallpaper {
        #[command(subcommand)]
        command: wallpaper::cli::WallpaperCommands,
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
    /// Update to the latest version
    SelfUpdate,
    /// Update system, dotfiles, and sync games
    Update,
    /// Run autostart tasks (setup assist, update dots, etc.)
    Autostart,
}

fn initialize_cli(cli: &Cli) {
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
        crate::ui::set_debug_mode(true);
    }
}

async fn dispatch_command(cli: &Cli) -> Result<()> {
    match &cli.command {
        Some(Commands::Arch { command }) => {
            execute_with_error_handling(
                arch::cli::handle_arch_command(command.clone(), cli.debug).await,
                "Error handling arch command",
                None,
            )?;
        }
        Some(Commands::Game { command }) => {
            execute_with_error_handling(
                game::handle_game_command(command.clone(), cli.debug),
                "Error handling game command",
                None,
            )?;
        }
        Some(Commands::Dot { command }) => {
            execute_with_error_handling(
                dot::commands::handle_dot_command(command, cli.config.as_deref(), cli.debug),
                "Error handling dot command",
                None,
            )?;
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
        Some(Commands::Assist { command }) => {
            execute_with_error_handling(
                assist::dispatch_assist_command(cli.debug, command.clone()),
                "Error handling assist command",
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
            execute_with_error_handling(
                settings::commands::handle_settings_command(
                    command,
                    setting,
                    category,
                    *search,
                    cli.debug,
                    cli.internal_privileged_mode,
                ),
                "Error running settings",
                None,
            )?;
        }
        Some(Commands::Video { command }) => {
            execute_with_error_handling(
                video::handle_video_command(command.clone(), cli.debug).await,
                "Error handling video command",
                None,
            )?;
        }
        Some(Commands::Wallpaper { command }) => {
            execute_with_error_handling(
                wallpaper::commands::handle_wallpaper_command(command.clone(), cli.debug).await,
                "Error handling wallpaper command",
                None,
            )?;
        }
        Some(Commands::Debug { command }) => {
            execute_with_error_handling(
                debug::handle_debug_command(command.clone()),
                "Error handling debug command",
                None,
            )?;
        }
        Some(Commands::Completions { command }) => {
            execute_with_error_handling(
                completions::handle_completions_command(command),
                "Error handling completions command",
                None,
            )?;
        }
        Some(Commands::SelfUpdate) => {
            execute_with_error_handling(
                self_update::self_update().await,
                "Error during self-update",
                None,
            )?;
        }
        Some(Commands::Update) => {
            execute_with_error_handling(
                update::handle_update_command(cli.debug).await,
                "Error during update",
                None,
            )?;
        }
        Some(Commands::Autostart) => {
            execute_with_error_handling(
                autostart::run(cli.debug).await,
                "Error running autostart",
                None,
            )?;
        }
        None => {
            Cli::command().print_help()?;
            println!();
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    clap_complete::CompleteEnv::with_factory(cli_command).complete();
    let cli = Cli::parse();
    initialize_cli(&cli);
    dispatch_command(&cli).await
}
