use anyhow::{Context, Result};
use std::process::Command;

pub fn apply_wallpaper(path: &str) -> Result<()> {
    Command::new("feh")
        .args(["--bg-fill", path])
        .output()
        .context("Failed to set wallpaper with feh")?;
    Ok(())
}
