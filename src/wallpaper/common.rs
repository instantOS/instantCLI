//! Common utilities for wallpaper generation
//!
//! Shared functions for ImageMagick, resolution detection, and overlay management.

use anyhow::{Context, Result};
use colored::*;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::common::compositor::CompositorType;

pub const OVERLAY_URL: &str =
    "https://raw.githubusercontent.com/instantOS/instantLOGO/main/wallpaper/overlay.png";

/// Get the wallpaper directory path
pub fn get_wallpaper_dir() -> Result<PathBuf> {
    let home = dirs::data_local_dir().context("Could not find local data directory")?;
    Ok(home.join("instant").join("wallpaper"))
}

/// Run ImageMagick command with fallback to 'convert' for IM6
pub fn run_magick(args: &[&str]) -> Result<()> {
    let status = Command::new("magick")
        .args(args)
        .status()
        .or_else(|_| Command::new("convert").args(args).status())
        .context("Failed to run ImageMagick command")?;

    if !status.success() {
        anyhow::bail!("ImageMagick command failed");
    }
    Ok(())
}

/// Detect current display resolution
pub fn get_resolution() -> Result<String> {
    let compositor = CompositorType::detect();
    match compositor {
        CompositorType::Sway => {
            let output = Command::new("swaymsg")
                .arg("-t")
                .arg("get_outputs")
                .output()?;
            let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            if let Some(outputs) = json.as_array() {
                for out in outputs {
                    if out["active"].as_bool().unwrap_or(false)
                        && let (Some(w), Some(h)) = (
                            out["rect"]["width"].as_i64(),
                            out["rect"]["height"].as_i64(),
                        )
                    {
                        return Ok(format!("{}x{}", w, h));
                    }
                }
            }
        }
        _ => {
            // Try xrandr for X11
            if let Ok(output) = Command::new("xrandr").output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let re = Regex::new(r"connected (?:primary )?([0-9]+x[0-9]+)")?;
                if let Some(caps) = re.captures(&stdout) {
                    return Ok(caps[1].to_string());
                }
            }
        }
    }
    anyhow::bail!("Could not detect resolution")
}

/// Ensure the overlay image exists, downloading if necessary
pub async fn ensure_overlay(dir: &Path) -> Result<PathBuf> {
    let overlay_path = dir.join("overlay.png");

    if !overlay_path.exists() {
        println!("{}", "Downloading overlay image...".cyan());
        let bytes = reqwest::get(OVERLAY_URL).await?.bytes().await?;
        let mut file = fs::File::create(&overlay_path).await?;
        file.write_all(&bytes).await?;
    }

    Ok(overlay_path)
}
