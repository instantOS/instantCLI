use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::time::Duration;

use crate::arch::engine::DataKey;

/// Fallback URL containing all HTTPS mirrors (commented out)
const ALL_HTTPS_MIRRORS_URL: &str = "https://archlinux.org/mirrorlist/all/https/";

/// Local mirrorlist path as last resort fallback
const LOCAL_MIRRORLIST_PATH: &str = "/etc/pacman.d/mirrorlist";

/// Maximum retry attempts for network requests
const MAX_RETRIES: u32 = 3;

// ============================================================================
// Data Keys
// ============================================================================

/// Key for storing the list of available mirror region names
pub struct MirrorRegionsKey;

impl DataKey for MirrorRegionsKey {
    type Value = Vec<String>;
    const KEY: &'static str = "mirror_regions";
}

/// Key to track whether mirror regions fetch failed
/// When true, the MirrorRegionQuestion should be skipped
pub struct MirrorRegionsFetchFailed;

impl DataKey for MirrorRegionsFetchFailed {
    type Value = bool;
    const KEY: &'static str = "mirror_regions_fetch_failed";
}

/// Key to track whether a fallback mirrorlist was used
/// This is set during execution when region-specific fetch fails
pub struct MirrorlistFallbackUsed;

impl DataKey for MirrorlistFallbackUsed {
    type Value = bool;
    const KEY: &'static str = "mirrorlist_fallback_used";
}

// ============================================================================
// Mirror Region Fetching
// ============================================================================

/// Fetch available mirror regions from archlinux.org with retry logic
pub async fn fetch_mirror_regions() -> Result<HashMap<String, String>> {
    let mut last_error = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_secs(2u64.pow(attempt));
            eprintln!(
                "Retrying mirror regions fetch in {}s (attempt {}/{})",
                delay.as_secs(),
                attempt + 1,
                MAX_RETRIES
            );
            tokio::time::sleep(delay).await;
        }

        match try_fetch_mirror_regions().await {
            Ok(regions) if !regions.is_empty() => return Ok(regions),
            Ok(_) => {
                last_error = Some(anyhow!("Received empty regions list from archlinux.org"));
            }
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch mirror regions after {} attempts", MAX_RETRIES)))
}

/// Single attempt to fetch mirror regions
async fn try_fetch_mirror_regions() -> Result<HashMap<String, String>> {
    let url = "https://archlinux.org/mirrorlist/";
    let response = reqwest::get(url).await?.text().await?;

    let mut regions = HashMap::new();

    // Simple parser for <option value="CODE">NAME</option>
    // We skip the first option which is usually "Select a country" or "All" if it has value=""
    for line in response.lines() {
        let line = line.trim();
        if line.starts_with("<option value=\"")
            && let Some(start_quote) = line.find('"')
            && let Some(end_quote) = line[start_quote + 1..].find('"')
        {
            let code = &line[start_quote + 1..start_quote + 1 + end_quote];

            if let Some(close_tag) = line.find('>')
                && let Some(end_tag) = line.find("</option>")
            {
                let name = &line[close_tag + 1..end_tag];

                if !code.is_empty() && !name.is_empty() && name != "All" {
                    regions.insert(name.to_string(), code.to_string());
                }
            }
        }
    }

    Ok(regions)
}

// ============================================================================
// Mirrorlist Fetching with Fallback Chain
// ============================================================================

/// Fetch mirrorlist with fallback chain:
/// 1. Region-specific mirrorlist (with retries)
/// 2. All HTTPS mirrors from archlinux.org
/// 3. Local /etc/pacman.d/mirrorlist
pub async fn fetch_mirrorlist(region_code: &str) -> Result<String> {
    // Try 1: Region-specific mirrorlist with retries
    if !region_code.is_empty() {
        match fetch_mirrorlist_with_retry(region_code).await {
            Ok(list) => return Ok(list),
            Err(e) => {
                eprintln!("Region-specific mirrorlist fetch failed: {}", e);
            }
        }
    }

    // Try 2: All HTTPS mirrors fallback
    eprintln!("Trying fallback: all HTTPS mirrors...");
    match fetch_all_https_mirrors().await {
        Ok(list) => return Ok(list),
        Err(e) => {
            eprintln!("All HTTPS mirrors fetch failed: {}", e);
        }
    }

    // Try 3: Local /etc/pacman.d/mirrorlist
    eprintln!("Trying fallback: local mirrorlist at {}...", LOCAL_MIRRORLIST_PATH);
    match std::fs::read_to_string(LOCAL_MIRRORLIST_PATH) {
        Ok(content) if !content.trim().is_empty() => {
            eprintln!("Using local mirrorlist as fallback");
            return Ok(content);
        }
        Ok(_) => {
            eprintln!("Local mirrorlist is empty");
        }
        Err(e) => {
            eprintln!("Failed to read local mirrorlist: {}", e);
        }
    }

    Err(anyhow!(
        "All mirrorlist sources failed. Please check your network connection or provide a valid /etc/pacman.d/mirrorlist"
    ))
}

/// Fetch region-specific mirrorlist with retry logic
async fn fetch_mirrorlist_with_retry(region_code: &str) -> Result<String> {
    let url = format!(
        "https://archlinux.org/mirrorlist/?country={}&protocol=https&ip_version=4",
        region_code
    );

    let mut last_error = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_secs(2u64.pow(attempt));
            tokio::time::sleep(delay).await;
        }

        match reqwest::get(&url).await {
            Ok(response) => match response.text().await {
                Ok(content) => {
                    let uncommented = uncomment_servers(&content);
                    if uncommented.contains("Server =") {
                        return Ok(uncommented);
                    }
                    last_error = Some(anyhow!("Mirrorlist contains no server entries"));
                }
                Err(e) => {
                    last_error = Some(e.into());
                }
            },
            Err(e) => {
                last_error = Some(e.into());
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch mirrorlist")))
}

/// Fetch all HTTPS mirrors as fallback
async fn fetch_all_https_mirrors() -> Result<String> {
    let mut last_error = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_secs(2u64.pow(attempt));
            tokio::time::sleep(delay).await;
        }

        match reqwest::get(ALL_HTTPS_MIRRORS_URL).await {
            Ok(response) => match response.text().await {
                Ok(content) => {
                    let uncommented = uncomment_servers(&content);
                    if uncommented.contains("Server =") {
                        return Ok(uncommented);
                    }
                    last_error = Some(anyhow!("All-mirrors list contains no server entries"));
                }
                Err(e) => {
                    last_error = Some(e.into());
                }
            },
            Err(e) => {
                last_error = Some(e.into());
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch all HTTPS mirrors")))
}

/// Uncomment server lines in mirrorlist content
fn uncomment_servers(content: &str) -> String {
    let mut mirrorlist = String::new();
    for line in content.lines() {
        if line.starts_with("#Server =") {
            mirrorlist.push_str(&line[1..]); // Remove leading #
        } else {
            mirrorlist.push_str(line);
        }
        mirrorlist.push('\n');
    }
    mirrorlist
}

// ============================================================================
// Data Provider
// ============================================================================

pub struct MirrorlistProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for MirrorlistProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        match fetch_mirror_regions().await {
            Ok(regions) => {
                let mut names: Vec<String> = regions.keys().cloned().collect();
                names.sort();
                context.set::<MirrorRegionsKey>(names);
                context.set::<MirrorRegionsFetchFailed>(false);
            }
            Err(e) => {
                eprintln!("Failed to fetch mirror regions: {}", e);
                eprintln!("Mirror region selection will be skipped; fallback mirrorlist will be used.");
                // Set empty list - the question will be skipped via should_ask()
                context.set::<MirrorRegionsKey>(Vec::new());
                context.set::<MirrorRegionsFetchFailed>(true);
            }
        }
        Ok(())
    }
}
