use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn command_exists(command: &str) -> bool {
    which::which(command).is_ok()
}

pub fn ensure_commands(commands: &[&str]) -> Result<()> {
    let missing = commands
        .iter()
        .copied()
        .filter(|command| !command_exists(command))
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        bail!("Missing required tools: {}", missing.join(", "));
    }

    Ok(())
}

pub fn run_status(command: &mut Command) -> Result<()> {
    let program = command.get_program().to_string_lossy().to_string();
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let output = command
        .output()
        .with_context(|| format!("Failed to execute {program}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{} {} failed: {}", program, args.join(" "), stderr.trim());
    }

    Ok(())
}

pub fn run_interactive_status(command: &mut Command) -> Result<()> {
    let program = command.get_program().to_string_lossy().to_string();
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let status = command
        .status()
        .with_context(|| format!("Failed to execute {program}"))?;

    if !status.success() {
        bail!("{} {} exited with {}", program, args.join(" "), status);
    }

    Ok(())
}
