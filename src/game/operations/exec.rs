use std::ffi::OsString;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::common::network::check_internet;

use super::sync::sync_game_saves;

/// Execute an arbitrary command with pre- and post-sync when internet is available.
pub fn exec_game_command(command: Vec<OsString>) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("No command provided to execute."));
    }

    let command_display = format_command(&command);

    if check_internet() {
        println!("Internet connection detected; syncing saves before launch...");
        sync_game_saves(None, false)?;
    } else {
        println!("No internet connection detected; skipping pre-launch sync.");
    }

    println!("Executing: {command_display}");

    let mut command_iter = command.into_iter();
    let program = command_iter
        .next()
        .expect("command vector is non-empty after validation");
    let args: Vec<OsString> = command_iter.collect();

    let mut process = Command::new(&program);
    if !args.is_empty() {
        process.args(&args);
    }

    let status = process
        .status()
        .with_context(|| format!("Failed to execute command: {command_display}"))?;

    if !status.success() {
        let exit_desc = status
            .code()
            .map(|code| format!("exited with code {code}"))
            .unwrap_or_else(|| "was terminated by signal".to_string());
        return Err(anyhow!("Command '{command_display}' {exit_desc}."));
    }

    if check_internet() {
        println!("Internet connection detected; syncing saves after exit...");
        sync_game_saves(None, false)?;
    } else {
        println!("No internet connection detected; skipping post-launch sync.");
    }

    println!("Finished exec workflow.");

    Ok(())
}

fn format_command(parts: &[OsString]) -> String {
    parts
        .iter()
        .map(|part| part.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn exec_requires_command() {
        let result = exec_game_command(Vec::new());
        assert!(result.is_err());
    }

    #[test]
    fn format_command_joins_parts() {
        let parts = vec![OsString::from("foo"), OsString::from("bar")];
        assert_eq!(format_command(&parts), "foo bar");
    }
}
