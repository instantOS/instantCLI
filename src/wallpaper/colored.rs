//! Colored wallpaper generation
//!
//! Creates solid-color wallpapers with the instantOS logo overlay.

use anyhow::Result;
use colored::*;
use std::path::PathBuf;
use tokio::fs;

use super::common::{ensure_overlay, get_resolution, get_wallpaper_dir, run_magick};

/// Options for generating a colored wallpaper
pub struct ColoredOptions {
    /// Background color in hex format (e.g., "#1a1a2e")
    pub bg_color: String,
    /// Foreground/logo color in hex format (e.g., "#ffffff")
    pub fg_color: String,
}

/// Generate a colored wallpaper with the instantOS logo
pub async fn generate_colored_wallpaper(options: ColoredOptions) -> Result<PathBuf> {
    let wallpaper_dir = get_wallpaper_dir()?;
    fs::create_dir_all(&wallpaper_dir).await?;

    let overlay_path = ensure_overlay(&wallpaper_dir).await?;

    let resolution = get_resolution().unwrap_or_else(|_| "1920x1080".to_string());
    println!("Target resolution: {}", resolution);
    println!(
        "Background: {}, Foreground: {}",
        options.bg_color.cyan(),
        options.fg_color.cyan()
    );

    let output_path = wallpaper_dir.join("instantwallpaper.png");

    // Generate the wallpaper
    let overlay = overlay_path.to_string_lossy().to_string();
    let out = output_path.to_string_lossy().to_string();
    let bg = options.bg_color;
    let fg = options.fg_color;
    let res = resolution;

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
    Ok(output_path)
}
