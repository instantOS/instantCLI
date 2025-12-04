//! Colored wallpaper generation
//!
//! Creates solid-color wallpapers with the instantOS logo overlay.

use anyhow::{Context, Result};
use colored::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::common::compositor::CompositorType;

const OVERLAY_URL: &str =
    "https://raw.githubusercontent.com/instantOS/instantLOGO/main/wallpaper/overlay.png";

/// Options for generating a colored wallpaper
pub struct ColoredOptions {
    /// Background color in hex format (e.g., "#1a1a2e")
    pub bg_color: String,
    /// Foreground/logo color in hex format (e.g., "#ffffff")
    pub fg_color: String,
}

/// Generate a colored wallpaper with the instantOS logo
pub async fn run(options: ColoredOptions) -> Result<PathBuf> {
    let wallpaper_dir = get_wallpaper_dir()?;
    fs::create_dir_all(&wallpaper_dir).await?;

    // Download overlay if missing
    let overlay_path = wallpaper_dir.join("overlay.png");
    if !overlay_path.exists() {
        println!("{}", "Downloading overlay image...".cyan());
        let bytes = reqwest::get(OVERLAY_URL).await?.bytes().await?;
        let mut file = fs::File::create(&overlay_path).await?;
        file.write_all(&bytes).await?;
    }

    let resolution = get_resolution().unwrap_or_else(|_| "1920x1080".to_string());
    println!("Target resolution: {}", resolution);
    println!(
        "Background: {}, Foreground: {}",
        options.bg_color.cyan(),
        options.fg_color.cyan()
    );

    let output_path = wallpaper_dir.join("instantwallpaper.png");

    // Generate the wallpaper
    generate_colored_wallpaper(
        &overlay_path,
        &options.bg_color,
        &options.fg_color,
        &resolution,
        &output_path,
    )
    .await?;

    Ok(output_path)
}

fn get_wallpaper_dir() -> Result<PathBuf> {
    let home = dirs::data_local_dir().context("Could not find local data directory")?;
    Ok(home.join("instant").join("wallpaper"))
}

async fn generate_colored_wallpaper(
    overlay_path: &Path,
    bg_color: &str,
    fg_color: &str,
    resolution: &str,
    output_path: &Path,
) -> Result<()> {
    let overlay = overlay_path.to_string_lossy().to_string();
    let out = output_path.to_string_lossy().to_string();
    let bg = bg_color.to_string();
    let fg = fg_color.to_string();
    let res = resolution.to_string();

    println!("{}", "Generating colored wallpaper...".cyan());

    tokio::task::spawn_blocking(move || {
        // Use ImageMagick to:
        // 1. Create a solid color background
        // 2. Load the overlay and resize it
        // 3. Extract the alpha channel as a mask
        // 4. Colorize the mask with the foreground color
        // 5. Composite onto the background
        run_magick(&[
            // Create solid background
            "-size",
            &res,
            &format!("xc:{}", bg),
            // Load and process overlay
            "(",
            &overlay,
            "-background",
            "none",
            "-resize",
            &format!("{}^", res),
            "-gravity",
            "center",
            "-extent",
            &res,
            // Extract alpha and colorize
            "-alpha",
            "extract",
            "-background",
            &fg,
            "-alpha",
            "shape",
            ")",
            // Composite
            "-gravity",
            "center",
            "-composite",
            &out,
        ])?;

        Ok::<(), anyhow::Error>(())
    })
    .await??;

    println!("{}", "Colored wallpaper generated!".green());
    Ok(())
}

fn run_magick(args: &[&str]) -> Result<()> {
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

fn get_resolution() -> Result<String> {
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
                let re = regex::Regex::new(r"connected (?:primary )?([0-9]+x[0-9]+)")?;
                if let Some(caps) = re.captures(&stdout) {
                    return Ok(caps[1].to_string());
                }
            }
        }
    }
    anyhow::bail!("Could not detect resolution")
}
