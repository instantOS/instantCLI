use anyhow::{Context, Result};
use colored::*;
use rand::RngExt;
use rand::seq::IndexedRandom;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;

use super::cli::WallpaperSource;
use super::common::{ensure_overlay, get_resolution, get_wallpaper_dir, run_magick};

const WALLHAVEN_API_URL: &str = "https://wallhaven.cc/api/v1/search?q=id%3A711&categories=111&purity=100&sorting=random&order=desc";
const BING_API_URL: &str = "https://www.bing.com/HPImageArchive.aspx";
const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Safari/537.36";

pub struct RandomOptions {
    pub no_logo: bool,
    /// The source to fetch from. `None` means auto: try the curated default
    /// first and fall back through the remaining sources, in
    /// [`default_source_chain`] order.
    pub source: Option<WallpaperSource>,
}

/// The sources tried when no explicit source was requested.
///
/// The curated instantOS collection on Wallhaven comes first because it is
/// the intended look; Bing's curated daily wallpaper follows, then the
/// generic photo services, so a single outage cannot leave the user without
/// a wallpaper.
fn default_source_chain() -> Vec<WallpaperSource> {
    vec![
        WallpaperSource::Wallhaven,
        WallpaperSource::Bing,
        WallpaperSource::Picsum,
        WallpaperSource::Loremflickr,
    ]
}

async fn fetch_from_source(source: WallpaperSource, wallpaper_dir: &Path) -> Result<PathBuf> {
    match source {
        WallpaperSource::Wallhaven => fetch_wallhaven_wallpaper(wallpaper_dir).await,
        WallpaperSource::Picsum => fetch_picsum_wallpaper(wallpaper_dir).await,
        WallpaperSource::Bing => fetch_bing_wallpaper(wallpaper_dir).await,
        WallpaperSource::Loremflickr => fetch_loremflickr_wallpaper(wallpaper_dir).await,
    }
}

pub async fn generate_random_wallpaper(options: RandomOptions) -> Result<PathBuf> {
    let wallpaper_dir = get_wallpaper_dir()?;
    fs::create_dir_all(&wallpaper_dir).await?;

    let chain = match options.source {
        Some(source) => vec![source],
        None => default_source_chain(),
    };

    let mut fetched = None;
    let mut last_error = None;
    for (index, source) in chain.iter().enumerate() {
        println!(
            "{}",
            format!("Fetching random wallpaper from {source}...").cyan()
        );
        match fetch_from_source(*source, &wallpaper_dir).await {
            Ok(path) => {
                fetched = Some(path);
                break;
            }
            Err(error) => {
                println!("{}", format!("{source} failed: {error}").yellow());
                if let Some(next) = chain.get(index + 1) {
                    println!("{}", format!("Falling back to {next}...").cyan());
                }
                last_error = Some(error);
            }
        }
    }

    let raw_image_path = match fetched {
        Some(path) => path,
        None => {
            let error =
                last_error.unwrap_or_else(|| anyhow::anyhow!("no wallpaper source was requested"));
            return Err(error.context("All wallpaper sources failed"));
        }
    };

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

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("ins/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client")
}

/// Returns true when the body is Wallhaven's outage placeholder instead of
/// real search results (it is served with HTTP 200, so status checks alone
/// do not catch it).
fn is_wallhaven_outage_page(body: &str) -> bool {
    body.contains("taking a little nap") || body.contains("wallhaven.cc Status")
}

fn wallhaven_outage_error(status: reqwest::StatusCode) -> anyhow::Error {
    anyhow::anyhow!(
        "Wallhaven is currently down (status: {status}). The site returns an outage page instead of wallpapers. Retry later or use another source: `ins wallpaper random --source picsum` (or bing, loremflickr)"
    )
}

fn target_dimensions() -> (u32, u32) {
    get_resolution()
        .ok()
        .and_then(|res| {
            let mut parts = res.split('x');
            match (parts.next()?.parse().ok(), parts.next()?.parse().ok()) {
                (Some(w), Some(h)) => Some((w, h)),
                _ => None,
            }
        })
        .unwrap_or((1920, 1080))
}

fn extension_for_response(resp: &reqwest::Response, fallback: &str) -> String {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| {
            if ct.contains("png") {
                Some("png".to_string())
            } else if ct.contains("webp") {
                Some("webp".to_string())
            } else if ct.contains("jpeg") || ct.contains("jpg") {
                Some("jpg".to_string())
            } else {
                None
            }
        })
        .or_else(|| {
            resp.url()
                .path()
                .rsplit('.')
                .next()
                .filter(|ext| ext.len() <= 4 && !ext.contains('/'))
                .map(|ext| ext.to_string())
        })
        .unwrap_or_else(|| fallback.to_string())
}

async fn download_image(client: &reqwest::Client, url: &str, path: &Path) -> Result<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to download image from {url}"))?
        .error_for_status()
        .with_context(|| format!("Image download failed for {url}"))?;
    let bytes = resp.bytes().await.context("Failed to read image bytes")?;
    if bytes.is_empty() {
        anyhow::bail!("Downloaded image from {url} was empty");
    }
    fs::write(path, &bytes)
        .await
        .context("Failed to save downloaded wallpaper")?;
    Ok(())
}

/// Fetch a random wallpaper via the official Wallhaven JSON API.
///
/// Uses `data[].path` (direct full-res image URL) instead of scraping HTML,
/// which is more robust against markup changes.
async fn fetch_wallhaven_wallpaper(dir: &Path) -> Result<PathBuf> {
    let client = build_client()?;

    let resp = client
        .get(WALLHAVEN_API_URL)
        .send()
        .await
        .context("Failed to reach Wallhaven API")?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .context("Failed to read Wallhaven response")?;

    if !status.is_success() || is_wallhaven_outage_page(&body) {
        return Err(wallhaven_outage_error(status));
    }

    let json: serde_json::Value =
        serde_json::from_str(&body).context("Failed to parse Wallhaven API response")?;
    let entries = json
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    if entries.is_empty() {
        anyhow::bail!(
            "Wallhaven returned no wallpapers for this search (empty `data` array). The collection may be empty or Wallhaven may have changed its API. Try again or use `--source picsum`"
        );
    }

    let entry = entries
        .choose(&mut rand::rng())
        .context("Failed to choose random Wallhaven entry")?;
    let img_url = entry
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Wallhaven API response has no `data[].path` image link (API shape may have changed). Try again or use `--source picsum`"
            )
        })?;

    println!("Downloading: {}", img_url);
    let ext = img_url
        .rsplit('.')
        .next()
        .unwrap_or("jpg")
        .split('?')
        .next()
        .unwrap_or("jpg");
    let output_path = dir.join(format!("wallhaven_raw.{}", ext));
    download_image(&client, img_url, &output_path).await?;

    Ok(output_path)
}

/// Fetch a random photo from picsum.photos.
///
/// `GET /{w}/{h}` returns a 302 to a Fastly CDN URL; reqwest follows it by
/// default. No API key needed.
async fn fetch_picsum_wallpaper(dir: &Path) -> Result<PathBuf> {
    let client = build_client()?;
    let (w, h) = target_dimensions();
    let url = format!("https://picsum.photos/{w}/{h}");

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to reach picsum.photos")?
        .error_for_status()
        .context("picsum.photos request failed")?;
    let ext = extension_for_response(&resp, "jpg");
    let bytes = resp.bytes().await.context("Failed to read picsum image")?;
    if bytes.is_empty() {
        anyhow::bail!("Downloaded image from picsum.photos was empty");
    }
    let output_path = dir.join(format!("picsum_raw.{}", ext));
    fs::write(&output_path, &bytes)
        .await
        .context("Failed to save picsum wallpaper")?;

    Ok(output_path)
}

/// Fetch a recent Bing daily wallpaper.
///
/// `HPImageArchive.aspx` returns JSON metadata; the image URL is built from
/// `urlbase` with a `_UHD.jpg` suffix (falls back to `url`). `idx` 0..7
/// covers the last 8 days, which keeps it "random". No API key needed.
async fn fetch_bing_wallpaper(dir: &Path) -> Result<PathBuf> {
    let client = build_client()?;
    let idx: u8 = rand::rng().random_range(0..8);
    let meta_url = format!("{BING_API_URL}?format=js&idx={idx}&n=1&mkt=en-US");

    let resp = client
        .get(&meta_url)
        .header(reqwest::header::USER_AGENT, BROWSER_UA)
        .send()
        .await
        .context("Failed to reach Bing wallpaper API")?
        .error_for_status()
        .context("Bing wallpaper API request failed")?;
    let json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse Bing wallpaper response")?;
    let image = json
        .get("images")
        .and_then(|i| i.as_array())
        .and_then(|i| i.first())
        .ok_or_else(|| anyhow::anyhow!("Bing API returned no images"))?;

    let urlbase = image.get("urlbase").and_then(|u| u.as_str());
    let fallback_url = image.get("url").and_then(|u| u.as_str());
    let mut candidates = Vec::new();
    if let Some(base) = urlbase {
        candidates.push(format!("https://www.bing.com{base}_UHD.jpg"));
        candidates.push(format!("https://www.bing.com{base}_1920x1080.jpg"));
    }
    if let Some(url) = fallback_url {
        candidates.push(format!("https://www.bing.com{url}"));
    }
    if candidates.is_empty() {
        anyhow::bail!("Bing API response has no image URL");
    }

    for url in &candidates {
        let attempt = client
            .get(url)
            .header(reqwest::header::USER_AGENT, BROWSER_UA)
            .send()
            .await;
        match attempt {
            Ok(resp) if resp.status().is_success() => {
                let ext = extension_for_response(&resp, "jpg");
                let bytes = resp.bytes().await.context("Failed to read Bing image")?;
                if bytes.is_empty() {
                    continue;
                }
                let output_path = dir.join(format!("bing_raw.{}", ext));
                fs::write(&output_path, &bytes)
                    .await
                    .context("Failed to save Bing wallpaper")?;
                println!("Downloading: {}", url);
                return Ok(output_path);
            }
            _ => continue,
        }
    }

    anyhow::bail!("Failed to download Bing wallpaper image (tried UHD/1080p URLs)")
}

/// Fetch a random photo from loremflickr.com.
///
/// `GET /{w}/{h}` returns a 302 (relative Location) to a cached JPEG;
/// reqwest follows it. Resolution is capped (~1280x720 in practice); the
/// overlay step resizes to the display resolution. No API key needed.
async fn fetch_loremflickr_wallpaper(dir: &Path) -> Result<PathBuf> {
    let client = build_client()?;
    let (w, h) = target_dimensions();
    let url = format!("https://loremflickr.com/{w}/{h}");

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to reach loremflickr.com")?
        .error_for_status()
        .context("loremflickr.com request failed")?;
    let ext = extension_for_response(&resp, "jpg");
    let bytes = resp
        .bytes()
        .await
        .context("Failed to read loremflickr image")?;
    if bytes.is_empty() {
        anyhow::bail!("Downloaded image from loremflickr.com was empty");
    }
    let output_path = dir.join(format!("loremflickr_raw.{}", ext));
    fs::write(&output_path, &bytes)
        .await
        .context("Failed to save loremflickr wallpaper")?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chain_prefers_curated_sources_over_generic_ones() {
        // The curated instantOS collection is the intended look and comes
        // first; Bing's curated wallpaper follows before the generic photo
        // services.
        assert_eq!(
            default_source_chain(),
            vec![
                WallpaperSource::Wallhaven,
                WallpaperSource::Bing,
                WallpaperSource::Picsum,
                WallpaperSource::Loremflickr,
            ]
        );
    }
}
