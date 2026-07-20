//! Command handling for welcome application

use std::io::IsTerminal;

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum WelcomeCommands {
    // No subcommands for now - welcome is a simple menu
}

pub fn handle_welcome_command(
    _command: &Option<WelcomeCommands>,
    gui: bool,
    force_live: bool,
    debug: bool,
) -> Result<()> {
    // Welcome is a TUI. Keep the terminal requirement at the command boundary
    // so desktop files and future graphical entry points cannot launch it
    // invisibly by mistake.
    if gui || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return launch_welcome_in_terminal(force_live, debug);
    }

    super::ui::run_welcome_ui(force_live, debug)
}

/// Launch the welcome UI in a terminal window
///
/// The terminal will automatically close when welcome exits.
/// Respects the user's $TERMINAL environment variable.
fn launch_welcome_in_terminal(force_live: bool, debug: bool) -> Result<()> {
    let args = terminal_args(force_live, debug);

    let current_exe = std::env::current_exe()?;
    let exe_str = current_exe.to_string_lossy();

    crate::common::terminal::TerminalLauncher::new(exe_str.as_ref())
        .class("ins-welcome")
        .title("Welcome to instantOS")
        .args(&args)
        .launch()
}

fn terminal_args(force_live: bool, debug: bool) -> Vec<String> {
    let mut args: Vec<String> = vec![];

    if debug {
        args.push("--debug".to_string());
    }

    args.push("welcome".to_string());

    if force_live {
        args.push("--force-live".to_string());
    }

    args
}

#[cfg(test)]
mod tests {
    use super::terminal_args;

    #[test]
    fn terminal_args_keep_subcommand_options_after_welcome() {
        assert_eq!(
            terminal_args(true, true),
            ["--debug", "welcome", "--force-live"]
        );
        assert_eq!(terminal_args(false, false), ["welcome"]);
    }
}
