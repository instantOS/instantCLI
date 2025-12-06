//! Command handling for welcome application

use anyhow::Result;
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

/// Launch the welcome UI in a terminal window
///
/// The terminal will automatically close when welcome exits.
/// Respects the user's $TERMINAL environment variable.
fn launch_welcome_in_terminal(debug: bool) -> Result<()> {
    let mut args: Vec<String> = vec![];

    if debug {
        args.push("--debug".to_string());
    }

    args.push("welcome".to_string());

    crate::common::terminal::launch_gui_terminal("ins-welcome", "Welcome to instantOS", &args)
}
