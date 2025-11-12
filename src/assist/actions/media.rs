use anyhow::{Context, Result};
use std::process::Command;

pub fn music() -> Result<()> {
    Command::new("playerctl")
        .arg("play-pause")
        .spawn()
        .context("Failed to control playback with playerctl")?;
    Ok(())
}

pub fn previous_track() -> Result<()> {
    Command::new("playerctl")
        .arg("previous")
        .spawn()
        .context("Failed to go to previous track with playerctl")?;
    Ok(())
}

pub fn next_track() -> Result<()> {
    Command::new("playerctl")
        .arg("next")
        .spawn()
        .context("Failed to go to next track with playerctl")?;
    Ok(())
}
