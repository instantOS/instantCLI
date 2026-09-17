use std::ffi::OsString;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use crate::common::network::check_internet;

use super::ui_dialog::{GameUi, after_launch};

/// Execute an arbitrary command with pre- and post-sync when internet is available.
pub fn exec_game_command(command: Vec<OsString>) -> Result<()> {
    let ui = GameUi::detect();
    let result = exec_with_ui(command, &ui);
    if let Err(error) = &result {
        ui.error(&format!("{error:#}"));
    }
    result
}

fn exec_with_ui(command: Vec<OsString>, ui: &GameUi) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("No command provided to execute."));
    }

    let command_display = format_command(&command);

    if check_internet() {
        println!("Internet connection detected; syncing saves before launch...");
        ui.sync("InstantCLI - Pre-launch Save Sync", Duration::ZERO)?
            .ensure_success()
            .context("Pre-launch save sync failed; command was not started")?;
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

    let launch_result = process
        .status()
        .with_context(|| format!("Failed to execute command: {command_display}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(anyhow!("Command '{command_display}' exited with {status}."))
            }
        });

    after_launch(launch_result, || {
        if check_internet() {
            println!("Internet connection detected; syncing saves after exit...");
            let report = ui.sync("InstantCLI - Post-exit Save Sync", Duration::ZERO)?;
            report
                .ensure_success()
                .context("Post-exit save sync failed")?;
            ui.completed(&report);
        } else {
            println!("No internet connection detected; skipping post-launch sync.");
        }
        Ok(())
    })?;

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
        let result = exec_with_ui(Vec::new(), &GameUi::terminal());
        assert!(result.is_err());
    }

    #[test]
    fn format_command_joins_parts() {
        let parts = vec![OsString::from("foo"), OsString::from("bar")];
        assert_eq!(format_command(&parts), "foo bar");
    }
}
