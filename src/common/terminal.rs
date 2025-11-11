/// Common terminal emulator utilities
use anyhow::Result;
use std::process::Command;

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
