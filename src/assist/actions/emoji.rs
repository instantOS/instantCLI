use anyhow::{Context, Result};
use std::process::Command;

pub fn emoji_picker() -> Result<()> {
    Command::new("flatpak")
        .args(["run", "com.tomjwatson.Emote"])
        .spawn()
        .context("Failed to launch Emote emoji picker")?;

    Ok(())
}
