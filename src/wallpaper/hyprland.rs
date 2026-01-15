use anyhow::{Context, Result};
use std::process::Command;

/// Apply wallpaper on Hyprland using swww
pub fn apply_wallpaper(path: &str) -> Result<()> {
    // Check if swww is installed
    if which::which("swww").is_err() {
        anyhow::bail!(
            "swww is not installed. Install it with: pacman -S swww\n\
             swww is required for wallpaper support on Hyprland."
        );
    }

    // Check if swww daemon is running by querying it
    let query = Command::new("swww")
        .arg("query")
        .output()
        .context("Failed to run swww query")?;

    // If daemon is not running, start it
    if !query.status.success() {
        Command::new("swww-daemon")
            .spawn()
            .context("Failed to start swww-daemon")?;

        // Give the daemon a moment to start
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Apply the wallpaper
    let output = Command::new("swww")
        .args(["img", path])
        .output()
        .context("Failed to set wallpaper with swww")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("swww failed to set wallpaper: {}", stderr);
    }

    Ok(())
}
