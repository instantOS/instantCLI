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

/// Run a TUI program (like cfdisk) from within an async context
///
/// Older TUI programs have issues when launched from within tokio's async runtime
/// because they can't properly acquire the terminal. This function works around
/// this by:
/// 1. Using tokio::task::spawn_blocking to run in a sync context
/// 2. Explicitly opening /dev/tty for stdin/stdout/stderr
/// 3. Handling SIGINT to allow graceful cancellation
///
/// Modern programs like fzf don't need this workaround.
///
/// # Arguments
/// * `program` - The program name (e.g., "cfdisk")
/// * `args` - Arguments to pass to the program
///
/// # Returns
/// * `Ok(true)` - Program exited successfully
/// * `Ok(false)` - Program was cancelled (Ctrl+C or non-zero exit)
/// * `Err` - Failed to run the program
///
/// # Example
/// ```ignore
/// if run_tui_program("cfdisk", &["/dev/sda"]).await? {
///     println!("cfdisk completed successfully");
/// }
/// ```
pub async fn run_tui_program(program: &str, args: &[&str]) -> Result<bool> {
    use std::fs::OpenOptions;
    use std::process::Stdio;

    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    // Register signal handler BEFORE spawning child to catch Ctrl+C
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    // Use spawn_blocking to run in a sync context
    // This avoids async runtime interference with terminal control
    let child_task = tokio::task::spawn_blocking(move || {
        // Open /dev/tty explicitly to ensure we have a valid terminal
        // This fixes issues where sudo/tokio might interfere with stdin/stdout inheritance
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("Failed to open /dev/tty - are you running from a terminal?")?;

        // We need separate handles for each stream
        let tty_in = tty
            .try_clone()
            .context("Failed to clone tty handle for stdin")?;
        let tty_out = tty
            .try_clone()
            .context("Failed to clone tty handle for stdout")?;
        let tty_err = tty
            .try_clone()
            .context("Failed to clone tty handle for stderr")?;

        let mut child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::from(tty_in))
            .stdout(Stdio::from(tty_out))
            .stderr(Stdio::from(tty_err))
            .spawn()
            .with_context(|| format!("Failed to spawn {}", program))?;

        // Wait for the program to complete
        let status = child
            .wait()
            .with_context(|| format!("Failed to wait for {}", program))?;

        Ok::<bool, anyhow::Error>(status.success())
    });

    tokio::select! {
        res = child_task => {
            match res {
                Ok(Ok(success)) => Ok(success),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(anyhow::anyhow!("Task join error: {}", e)),
            }
        }
        _ = sigint.recv() => {
            // User pressed Ctrl+C
            Ok(false)
        }
    }
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
