use std::io::ErrorKind;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub(super) fn is_flatpak_app_installed(app_id: &str) -> Result<bool> {
    let output = match Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("Failed to query Flatpak applications"),
    };
    if !output.status.success() {
        bail!(
            "Failed to query Flatpak applications: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim() == app_id))
}
