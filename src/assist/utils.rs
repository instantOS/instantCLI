/// Shared utility functions for assists
use anyhow::{Context, Result};
use std::process::Command;

/// Launch a command in a detached terminal window
pub fn launch_in_terminal(command: &str) -> Result<()> {
    Command::new("kitty")
        .args(["-e", "bash", "-c", command])
        .spawn()
        .context("Failed to launch terminal")?;
    Ok(())
}

/// Launch a command in the background (detached)
pub fn launch_detached(program: &str, args: &[&str]) -> Result<()> {
    Command::new(program)
        .args(args)
        .spawn()
        .context(format!("Failed to launch {} in background", program))?;
    Ok(())
}

/// Get the current executable path (useful for calling self)
pub fn current_exe() -> Result<std::path::PathBuf> {
    std::env::current_exe().context("Failed to get current executable path")
}

/// Execute an ins menu command
pub fn menu_command(args: &[&str]) -> Result<()> {
    Command::new(current_exe()?)
        .arg("menu")
        .args(args)
        .spawn()
        .context("Failed to execute menu command")?;
    Ok(())
}
