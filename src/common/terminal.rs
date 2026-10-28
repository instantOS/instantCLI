/// Common terminal launching, backed by the shared instantOS policy.
use anyhow::{Context, Result};
use instantos_terminal::Launcher;
use std::process::Command;

/// Launch a menu subcommand (`ins <module> menu`) in a GUI terminal window.
///
/// This is the shared implementation behind `--gui` on menu commands.
/// `module` is the top-level subcommand name (e.g. "game", "dot", "video").
/// `extra_args` are any additional CLI arguments forwarded to the menu
/// (e.g. a game name).
pub fn launch_menu_in_terminal(
    module: &str,
    title: &str,
    extra_args: &[String],
    debug: bool,
) -> Result<()> {
    let mut args: Vec<String> = vec![];

    if debug {
        args.push("--debug".to_string());
    }

    args.push(module.to_string());
    args.push("menu".to_string());
    args.extend_from_slice(extra_args);

    let current_exe = std::env::current_exe()?;
    Launcher::new(current_exe.as_os_str())
        .app_id(format!("ins-{module}"))
        .title(title)
        .args(&args)
        .spawn_detached()
        .map(|_| ())
        .map_err(Into::into)
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
