use anyhow::{Context, Result};
use std::process::Command;

pub fn apply_wallpaper(path: &str) -> Result<()> {
    let status = Command::new("instantwmctl")
        .args(["wallpaper", path])
        .status()
        .context("Failed to set wallpaper with instantwmctl")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "instantwmctl wallpaper failed with exit code {}",
            status.code().unwrap_or(-1)
        )
    }
}
