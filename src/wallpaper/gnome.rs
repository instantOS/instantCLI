use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn apply_wallpaper(path: &str) -> Result<()> {
    let abs_path = Path::new(path)
        .canonicalize()
        .context("Failed to resolve absolute path for wallpaper")?;

    let uri = format!("'file://{}'", abs_path.display());

    Command::new("dconf")
        .args(["write", "/org/gnome/desktop/background/picture-uri", &uri])
        .output()
        .context("Failed to set wallpaper with dconf")?;

    Command::new("dconf")
        .args([
            "write",
            "/org/gnome/desktop/background/picture-uri-dark",
            &uri,
        ])
        .output()
        .context("Failed to set dark wallpaper with dconf")?;

    Command::new("dconf")
        .args([
            "write",
            "/org/gnome/desktop/background/picture-options",
            "'zoom'",
        ])
        .output()
        .context("Failed to set wallpaper options with dconf")?;

    Ok(())
}
