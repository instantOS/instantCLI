use anyhow::{Context, Result};
use std::process::Command;

use crate::settings::deps::AWWW;

/// Apply wallpaper using awww (formerly swww).
///
/// Works on any Wayland compositor that implements wlr-layer-shell, including
/// Hyprland and niri.
pub fn apply_wallpaper(path: &str) -> Result<()> {
    if which::which("awww").is_err() {
        let hint = AWWW.install_hint();
        anyhow::bail!(
            "awww is not installed.\n\n{}\n\nawww is required for wallpaper support on this compositor.",
            hint
        );
    }

    let query = Command::new("awww")
        .arg("query")
        .output()
        .context("Failed to run awww query")?;

    if !query.status.success() {
        // Daemonize properly so it survives the parent terminal closing.
        // `awww-daemon` forks but stays in the same session by default; closing
        // the terminal sends SIGHUP to the session and kills it, reverting the
        // wallpaper. Use nohup + setsid to detach.
        Command::new("sh")
            .args(["-c", "nohup awww-daemon >/dev/null 2>&1 &"])
            .output()
            .context("Failed to start awww-daemon")?;

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let output = Command::new("awww")
        .args(["img", path])
        .output()
        .context("Failed to set wallpaper with awww")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("awww failed to set wallpaper: {}", stderr);
    }

    Ok(())
}
