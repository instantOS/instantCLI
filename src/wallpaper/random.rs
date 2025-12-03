use anyhow::{Context, Result};
use colored::*;
use rand::seq::SliceRandom;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::common::compositor::CompositorType;

const WALLHAVEN_SEARCH_URL: &str =
    "https://wallhaven.cc/search?q=id%3A711&categories=111&purity=100&sorting=random&order=desc";
const OVERLAY_URL: &str =
    "https://raw.githubusercontent.com/instantOS/instantLOGO/main/wallpaper/overlay.png";

pub struct RandomOptions {
    pub no_logo: bool,
}

pub async fn run(options: RandomOptions) -> Result<PathBuf> {
    let wallpaper_dir = get_wallpaper_dir()?;
    fs::create_dir_all(&wallpaper_dir).await?;

    println!("{}", "Fetching random wallpaper from Wallhaven...".cyan());
    let raw_image_path = fetch_random_wallhaven_wallpaper(&wallpaper_dir).await?;

    let final_path = if options.no_logo {
        println!("{}", "Skipping logo overlay...".yellow());
        let dest = wallpaper_dir.join("instantwallpaper.png");
        fs::copy(&raw_image_path, &dest).await?;
        dest
    } else {
        println!("{}", "Applying instantOS logo overlay...".cyan());
        apply_overlay(&raw_image_path, &wallpaper_dir).await?
    };

    Ok(final_path)
}

fn get_wallpaper_dir() -> Result<PathBuf> {
    let home = dirs::data_local_dir().context("Could not find local data directory")?;
    Ok(home.join("instant").join("wallpaper"))
}

async fn fetch_random_wallhaven_wallpaper(dir: &Path) -> Result<PathBuf> {
    let client = reqwest::Client::new();

    // Step 1: Search page
    let resp = client
        .get(WALLHAVEN_SEARCH_URL)
        .send()
        .await?
        .text()
        .await?;

    // Extract wallpaper page links
    let re_wall_link = Regex::new(r#"https://wallhaven.cc/w/[a-z0-9]+"#)?;
    let links: Vec<&str> = re_wall_link.find_iter(&resp).map(|m| m.as_str()).collect();

    if links.is_empty() {
        anyhow::bail!("No wallpapers found on Wallhaven search page");
    }

    let selected_link = links
        .choose(&mut rand::thread_rng())
        .context("Failed to choose random wallpaper link")?;

    // Step 2: Wallpaper page
    let resp = client.get(*selected_link).send().await?.text().await?;

    // Extract full image link
    let re_img_link = Regex::new(r#"https://w.wallhaven.cc/full/[^"]+\.(jpg|png)"#)?;
    let img_link = re_img_link
        .find(&resp)
        .context("Could not find full image link on wallpaper page")?
        .as_str();

    // Step 3: Download image
    println!("Downloading: {}", img_link);
    let img_resp = client.get(img_link).send().await?;
    let bytes = img_resp.bytes().await?;

    let ext = Path::new(img_link)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("jpg");
    let file_path = dir.join(format!("downloaded.{}", ext));

    let mut file = fs::File::create(&file_path).await?;
    file.write_all(&bytes).await?;

    Ok(file_path)
}

async fn apply_overlay(bg_path: &Path, dir: &Path) -> Result<PathBuf> {
    let overlay_path = dir.join("overlay.png");

    // Download overlay if missing
    if !overlay_path.exists() {
        println!("Downloading overlay image...");
        let bytes = reqwest::get(OVERLAY_URL).await?.bytes().await?;
        let mut file = fs::File::create(&overlay_path).await?;
        file.write_all(&bytes).await?;
    }

    let resolution = get_resolution().unwrap_or_else(|_| "1920x1080".to_string());
    println!("Target resolution: {}", resolution);

    let output_path = dir.join("instantwallpaper.png");

    // Clone paths to move into the closure
    let bg_path_buf = bg_path.to_path_buf();
    let overlay_path_buf = overlay_path.to_path_buf();
    let output_path_buf = output_path.to_path_buf();

    let _status = tokio::task::spawn_blocking(move || {
        let bg = bg_path_buf.to_string_lossy();
        let overlay = overlay_path_buf.to_string_lossy();
        let out = output_path_buf.to_string_lossy();

        // Use a single modern ImageMagick command to process everything
        // This avoids deprecated 'convert' subcommand and temporary files
        // Logic (matching working bash implementation):
        // 1. Load and resize background
        // 2. Load and resize overlay, extract alpha (mask)
        // 3. Clone BG and apply CopyOpacity with mask to create cutout, then Negate RGB to invert colors
        // 4. Delete the mask (index 1)
        // 5. Composite the Inverted Cutout over the original BG
        run_magick(&[
            // 1. Load and process Background (Dest)
            &bg,
            "-resize",
            &format!("{}^", resolution),
            "-gravity",
            "center",
            "-extent",
            &resolution,
            // 2. Create Inverted Background (Source)
            "(",
            "-clone",
            "0",
            "-negate",
            ")",
            // 3. Load Overlay and create Mask (Mask)
            "(",
            &overlay,
            "-background",
            "none", // Ensure background is transparent for resize/extent
            "-resize",
            &format!("{}^", resolution),
            "-gravity",
            "center",
            "-extent",
            &resolution,
            "-alpha",
            "extract",
            ")",
            // 4. Composite Source over Dest using Mask
            // This blends the Inverted BG onto the Original BG based on the mask opacity
            // Preserves anti-aliasing and soft edges correctly
            "-compose",
            "Over",
            "-composite",
            &out,
        ])?;

        Ok::<(), anyhow::Error>(())
    })
    .await??;

    Ok(output_path)
}

fn run_magick(args: &[&str]) -> Result<()> {
    let status = Command::new("magick")
        .args(args)
        .status()
        .or_else(|_| Command::new("convert").args(args).status()) // Fallback to convert if magick not found (IM6)
        .context("Failed to run ImageMagick command")?;

    if !status.success() {
        anyhow::bail!("ImageMagick command failed: {:?}", args);
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
            // Get first active output
            if let Some(outputs) = json.as_array() {
                for out in outputs {
                    if out["active"].as_bool().unwrap_or(false) {
                        if let (Some(w), Some(h)) = (
                            out["rect"]["width"].as_i64(),
                            out["rect"]["height"].as_i64(),
                        ) {
                            return Ok(format!("{}x{}", w, h));
                        }
                    }
                }
            }
        }
        _ => {
            // Try xrandr for X11
            if let Ok(output) = Command::new("xrandr").output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Look for connected primary or just connected
                // Example: "HDMI-1 connected primary 1920x1080+0+0"
                let re = Regex::new(r"connected (?:primary )?([0-9]+x[0-9]+)")?;
                if let Some(caps) = re.captures(&stdout) {
                    return Ok(caps[1].to_string());
                }
            }
        }
    }
    anyhow::bail!("Could not detect resolution")
}
