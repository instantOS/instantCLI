//! Ludusavi manifest download and caching

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};

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

    let response = request
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

    let body = response
        .bytes()
        .context("Failed to read Ludusavi manifest body")?;

    fs::create_dir_all(cache_path.parent().unwrap())?;
    let mut file = fs::File::create(&cache_path)?;
    file.write_all(&body)?;

    if let Some(etag) = new_etag {
        let _ = save_etag(&etag);
    }

    Ok(true)
}

/// Load the manifest from cache, downloading if needed.
pub fn load_manifest() -> Result<LudusaviManifest> {
    let cache_path = manifest_cache_path()?;

    // Download if not cached
    if !cache_path.exists() {
        download_manifest()?;
    }

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
                if let Ok(modified) = meta.modified() {
                    if let Ok(elapsed) = modified.elapsed() {
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
                }
                "Cached".to_string()
            }
            Err(_) => "Cached (stat failed)".to_string(),
        }
    } else {
        "Not cached".to_string()
    }
}
