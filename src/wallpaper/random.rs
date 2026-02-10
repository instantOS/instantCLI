use anyhow::Result;
use colored::*;
use rand::seq::SliceRandom;
use regex::Regex;
use std::path::{Path, PathBuf};
use tokio::fs;

use super::common::{ensure_overlay, get_resolution, get_wallpaper_dir, run_magick};

const WALLHAVEN_SEARCH_URL: &str =
    "https://wallhaven.cc/search?q=id%3A711&categories=111&purity=100&sorting=random&order=desc";

pub struct RandomOptions {
    pub no_logo: bool,
}

pub async fn generate_random_wallpaper(options: RandomOptions) -> Result<PathBuf> {
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
        anyhow::bail!("No wallpaper links found on Wallhaven search page");
    }

    let wall_page_url = links.choose(&mut rand::thread_rng()).unwrap();

    // Step 2: Wallpaper page
    let wall_resp = client.get(*wall_page_url).send().await?.text().await?;

    // Extract direct image link
    let re_img_link =
        Regex::new(r#"https://w\.wallhaven\.cc/full/[a-z0-9]+/wallhaven-[a-z0-9]+\.[a-z]+"#)?;
    let img_url = re_img_link
        .find(&wall_resp)
        .map(|m| m.as_str())
        .ok_or_else(|| anyhow::anyhow!("Could not find direct image link on wallpaper page"))?;

    // Step 3: Download image
    let ext = img_url
        .rsplit('.')
        .next()
        .unwrap_or("png")
        .split('?')
        .next()
        .unwrap_or("png");
    let output_path = dir.join(format!("wallhaven_raw.{}", ext));

    let img_bytes = client.get(img_url).send().await?.bytes().await?;
    fs::write(&output_path, &img_bytes).await?;

    Ok(output_path)
}

async fn apply_overlay(bg_path: &Path, dir: &Path) -> Result<PathBuf> {
    let overlay_path = ensure_overlay(dir).await?;

    let resolution = get_resolution().unwrap_or_else(|_| "1920x1080".to_string());
    println!("Target resolution: {}", resolution);

    let output_path = dir.join("instantwallpaper.png");

    // Clone paths to move into the closure
    let bg_path_buf = bg_path.to_path_buf();
    let overlay_path_buf = overlay_path.to_path_buf();
    let output_path_buf = output_path.to_path_buf();

    tokio::task::spawn_blocking(move || {
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
