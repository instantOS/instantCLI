//! Ludusavi manifest download and caching

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use super::types::LudusaviManifest;

const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";
const CACHE_DIR_NAME: &str = "instant";
const MANIFEST_FILE: &str = "ludusavi-manifest.yaml";
const ETAG_FILE: &str = "ludusavi-manifest.etag";

/// Get the cache directory path for ludusavi data
fn cache_dir() -> Result<PathBuf> {
    let mut path = dirs::cache_dir().context("Unable to resolve cache directory")?;
    path.push(CACHE_DIR_NAME);
    Ok(path)
}

/// Get the full path to the cached manifest file
pub fn manifest_cache_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join(MANIFEST_FILE))
}

/// Get the full path to the cached ETag file
fn etag_cache_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join(ETAG_FILE))
}

/// Read the cached ETag value, if present
fn read_cached_etag() -> Option<String> {
    let path = etag_cache_path().ok()?;
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Save the ETag value to cache
fn save_etag(etag: &str) -> Result<()> {
    let path = etag_cache_path()?;
    fs::create_dir_all(path.parent().unwrap())?;
    let mut file = fs::File::create(&path)?;
    file.write_all(etag.as_bytes())?;
    Ok(())
}

/// Download the manifest, using ETag if available.
/// Returns true if a new manifest was downloaded.
fn download_manifest() -> Result<bool> {
    let cache_path = manifest_cache_path()?;
    let cached_etag = read_cached_etag();

    let client = reqwest::blocking::Client::new();
    let mut request = client.get(MANIFEST_URL);

    if let Some(ref etag) = cached_etag {
        request = request.header("If-None-Match", etag);
    }

    let mut response = request
        .send()
        .context("Failed to request Ludusavi manifest")?;

    if response.status() == 304 {
        // Not modified — cached version is current
        return Ok(false);
    }

    let status = response.status();
    if !status.is_success() {
        // If we have a cached version, use it
        if cache_path.exists() {
            return Ok(false);
        }
        return Err(anyhow::anyhow!(
            "Failed to download Ludusavi manifest: HTTP {}",
            status
        ));
    }

    // Extract new ETag
    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    fs::create_dir_all(cache_path.parent().unwrap())?;
    let mut file = fs::File::create(&cache_path)?;
    let progress = create_download_progress(response.content_length());
    let mut downloaded = 0u64;
    let mut buffer = [0u8; 16 * 1024];

    loop {
        let bytes_read = response
            .read(&mut buffer)
            .context("Failed to read Ludusavi manifest body")?;
        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])
            .context("Failed to write Ludusavi manifest cache")?;

        downloaded += bytes_read as u64;
        progress.set_position(downloaded);
    }

    progress.finish_with_message("Download complete");

    if let Some(etag) = new_etag {
        let _ = save_etag(&etag);
    }

    Ok(true)
}

fn create_download_progress(total_size: Option<u64>) -> ProgressBar {
    let pb = if let Some(size) = total_size {
        ProgressBar::new(size)
    } else {
        ProgressBar::new_spinner()
    };

    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    pb.set_message("Downloading Ludusavi manifest");
    pb
}

fn ensure_manifest_cached(cache_path: &std::path::Path) -> Result<()> {
    if cache_path.exists() {
        return Ok(());
    }

    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(download_manifest)?;
    } else {
        download_manifest()?;
    }

    Ok(())
}

/// Load the manifest from cache, downloading if needed.
pub fn load_manifest() -> Result<LudusaviManifest> {
    let cache_path = manifest_cache_path()?;

    ensure_manifest_cached(&cache_path)?;

    let yaml = fs::read_to_string(&cache_path)
        .with_context(|| format!("Failed to read cached manifest at {}", cache_path.display()))?;

    let manifest: LudusaviManifest =
        serde_yaml::from_str(&yaml).context("Failed to parse Ludusavi manifest YAML")?;

    Ok(manifest)
}

/// Check if the manifest is available (cached or downloadable)
#[allow(dead_code)]
pub fn is_manifest_available() -> bool {
    load_manifest().is_ok()
}

/// Get the last update status message
pub fn manifest_status() -> String {
    let cache_path = match manifest_cache_path() {
        Ok(p) => p,
        Err(_) => return "Cache directory unavailable".to_string(),
    };

    if cache_path.exists() {
        match fs::metadata(&cache_path) {
            Ok(meta) => {
                if let Ok(modified) = meta.modified()
                    && let Ok(elapsed) = modified.elapsed()
                {
                    let hours = elapsed.as_secs() / 3600;
                    if hours < 1 {
                        let mins = elapsed.as_secs() / 60;
                        return format!("Updated {} min ago", mins);
                    } else if hours < 24 {
                        return format!("Updated {}h ago", hours);
                    } else {
                        let days = hours / 24;
                        return format!("Updated {}d ago", days);
                    }
                }
                "Cached".to_string()
            }
            Err(_) => "Cached (stat failed)".to_string(),
        }
    } else {
        "Not cached".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_manifest_cached_is_noop_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("manifest.yaml");
        fs::write(&cache_path, "dummy").unwrap();

        ensure_manifest_cached(&cache_path).unwrap();

        assert_eq!(fs::read_to_string(&cache_path).unwrap(), "dummy");
    }
}
