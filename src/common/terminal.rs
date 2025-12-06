/// Common terminal emulator utilities
use anyhow::{Context, Result};
use std::process::Command;

use crate::scratchpad::terminal::Terminal;

/// Detect the available terminal emulator
///
/// Checks for common terminals in order of preference, respecting the $TERMINAL
/// environment variable if set.
pub fn detect_terminal() -> String {
    // First check environment variable
    if let Ok(term) = std::env::var("TERMINAL")
        && !term.is_empty()
        && is_available(&term)
    {
        return term;
    }

    // Common terminal emulators in order of preference
    const TERMINALS: &[&str] = &[
        "kitty",
        "alacritty",
        "wezterm",
        "foot",
        "gnome-terminal",
        "konsole",
        "xterm",
    ];

    for terminal in TERMINALS {
        if is_available(terminal) {
            return terminal.to_string();
        }
    }

    // Fallback
    "xterm".to_string()
}

/// Check if a terminal emulator is available
fn is_available(terminal: &str) -> bool {
    which::which(terminal).is_ok()
}

/// Get the execute flag for a terminal
///
/// Returns the flag needed to execute a command in the terminal (e.g., "-e")
#[allow(dead_code)]
pub fn get_execute_flag(terminal: &str) -> &'static str {
    match terminal {
        "kitty" | "alacritty" | "wezterm" | "foot" | "xterm" => "-e",
        "gnome-terminal" | "konsole" => "-e",
        _ => "-e", // Assume standard -e flag
    }
}

/// Wrap a command to run in a terminal
///
/// Modifies the command to run inside a terminal emulator.
pub fn wrap_with_terminal(cmd: &mut Command) -> Result<()> {
    let terminal = detect_terminal();

    // Build the terminal command
    let mut term_cmd = Command::new(&terminal);

    // Add terminal-specific arguments
    match terminal.as_str() {
        "kitty" | "alacritty" => {
            term_cmd.arg("--");
        }
        "gnome-terminal" => {
            term_cmd.arg("--");
        }
        _ => {}
    }

    // Add the original command as arguments to the terminal
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    term_cmd.arg(program);
    for arg in args {
        term_cmd.arg(arg);
    }

    // Replace the original command with the terminal-wrapped version
    *cmd = term_cmd;

    Ok(())
}

/// Launch a GUI terminal window for running ins subcommands
///
/// This function spawns a new terminal window with the specified window class and title,
/// executing the current binary with the provided arguments. The terminal is auto-detected
/// using `detect_terminal()`, respecting the user's `$TERMINAL` environment variable.
///
/// # Arguments
/// * `class` - Window class name for the terminal (e.g., "ins-settings", "ins-welcome")
/// * `title` - Window title to display
/// * `args` - Arguments to pass to the ins binary
///
/// # Example
/// ```ignore
/// launch_gui_terminal(
///     "ins-settings",
///     "Settings",
///     &["settings", "--category", "system"]
/// )?;
/// ```
pub fn launch_gui_terminal(class: &str, title: &str, args: &[String]) -> Result<()> {
    let terminal_str = detect_terminal();
    let terminal: Terminal = terminal_str.as_str().into();
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;

    let mut cmd = Command::new(terminal.command());

    // Add class flag (all common terminals support this)
    let class_flag = terminal.class_flag(class);
    for part in class_flag.split_whitespace() {
        cmd.arg(part);
    }

    // Add title flag (kitty, alacritty, wezterm support this)
    match terminal {
        Terminal::Kitty | Terminal::Alacritty | Terminal::Wezterm => {
            cmd.arg("--title");
            cmd.arg(title);
        }
        _ => {
            // Other terminals may not support --title in the same way
        }
    }

    // Add separator before command (standard for modern terminals)
    cmd.arg("--");

    // Add the ins binary and its arguments
    cmd.arg(&current_exe);
    cmd.args(args);

    cmd.spawn()
        .context("Failed to launch terminal for GUI mode")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_terminal_returns_something() {
        let terminal = detect_terminal();
        assert!(!terminal.is_empty());
    }

    #[test]
    fn test_get_execute_flag() {
        assert_eq!(get_execute_flag("kitty"), "-e");
        assert_eq!(get_execute_flag("alacritty"), "-e");
        assert_eq!(get_execute_flag("xterm"), "-e");
        assert_eq!(get_execute_flag("unknown"), "-e");
    }
}
