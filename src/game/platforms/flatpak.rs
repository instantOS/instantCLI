use std::process::Command;

use anyhow::Result;

pub(super) fn is_flatpak_app_installed(app_id: &str) -> Result<bool> {
    let output = Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.lines().any(|line| line.trim() == app_id))
        }
        Err(_) => Ok(false),
    }
}
