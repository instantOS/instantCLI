//! Command handling for welcome application

use anyhow::{Context, Result};
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum WelcomeCommands {
    // No subcommands for now - welcome is a simple menu
}

pub fn handle_welcome_command(
    _command: &Option<WelcomeCommands>,
    gui: bool,
    debug: bool,
) -> Result<()> {
    if gui {
        return launch_welcome_in_terminal(debug);
    }

    super::ui::run_welcome_ui(debug)
}

/// Launch the welcome UI in a kitty terminal window
///
/// The terminal will automatically close when welcome exits.
fn launch_welcome_in_terminal(debug: bool) -> Result<()> {
    use std::process::Command;

    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;

    let mut args: Vec<String> = vec!["welcome".to_string()];

    if debug {
        args.insert(0, "--debug".to_string());
    }

    // Launch kitty with welcome
    Command::new("kitty")
        .arg("--class")
        .arg("ins-welcome")
        .arg("--title")
        .arg("Welcome to instantOS")
        .arg("--")
        .arg(&current_exe)
        .args(&args)
        .spawn()
        .context("Failed to launch kitty terminal for welcome")?;

    Ok(())
}
