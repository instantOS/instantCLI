use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// Key for the region-name -> country-code map from archlinux.org.
///
/// [`MirrorRegionsKey`] keeps only the display names for the question list;
/// this preserves the codes so a detected country can be mapped back to its
/// region without re-fetching.
pub struct MirrorRegionCodesKey;

impl DataKey for MirrorRegionCodesKey {
    type Value = HashMap<String, String>;
    const KEY: &'static str = "mirror_region_codes";
}

/// Key to track whether mirror regions fetch failed
/// When true, the MirrorRegionQuestion should be skipped
pub struct MirrorRegionsFetchFailed;

impl DataKey for MirrorRegionsFetchFailed {
    type Value = bool;
    const KEY: &'static str = "mirror_regions_fetch_failed";
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

    Err(last_error.unwrap_or_else(|| {
        anyhow!(
            "Failed to fetch mirror regions after {} attempts",
            MAX_RETRIES
        )
    }))
}

/// Single attempt to fetch mirror regions
async fn try_fetch_mirror_regions() -> Result<HashMap<String, String>> {
    let url = "https://archlinux.org/mirrorlist/";
    let response = download_text(url).await?;
    Ok(parse_region_options(&response))
}

/// Parse the archlinux.org mirrorlist page's `<option value="CODE">NAME</option>`
/// entries into a name -> country-code map. Shared by the live fetch and the
/// offline bundle's `regions.html` snapshot so a page change breaks both
/// paths identically.
fn parse_region_options(response: &str) -> HashMap<String, String> {
    let mut regions = HashMap::new();

    // Entries without a value or name are skipped, as is the "All" pseudo-entry.
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

    regions
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
            Ok(list) => match validate_and_prioritize_mirrors(&list).await {
                Ok(list) => return Ok(list),
                Err(e) => {
                    eprintln!("No usable mirror found in the selected region: {e:#}");
                }
            },
            Err(e) => {
                eprintln!("Region-specific mirrorlist fetch failed: {}", e);
            }
        }
    }

    // Try 2: All HTTPS mirrors fallback
    eprintln!("Trying fallback: all HTTPS mirrors...");
    match fetch_all_https_mirrors().await {
        Ok(list) => match validate_and_prioritize_mirrors(&list).await {
            Ok(list) => return Ok(list),
            Err(e) => {
                eprintln!("No usable mirror found in the HTTPS fallback list: {e:#}");
            }
        },
        Err(e) => {
            eprintln!("All HTTPS mirrors fetch failed: {}", e);
        }
    }

    // Try 3: Local /etc/pacman.d/mirrorlist
    eprintln!(
        "Trying fallback: local mirrorlist at {}...",
        LOCAL_MIRRORLIST_PATH
    );
    match std::fs::read_to_string(LOCAL_MIRRORLIST_PATH) {
        Ok(content) if !content.trim().is_empty() => {
            match validate_and_prioritize_mirrors(&content).await {
                Ok(content) => {
                    eprintln!("Using validated local mirrorlist as fallback");
                    return Ok(content);
                }
                Err(e) => {
                    eprintln!("Local mirrorlist has no usable mirror: {e:#}");
                }
            }
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

        match download_text(&url).await {
            Ok(content) => {
                let uncommented = uncomment_servers(&content);
                if uncommented.contains("Server =") {
                    return Ok(uncommented);
                }
                last_error = Some(anyhow!("Mirrorlist contains no server entries"));
            }
            Err(e) => {
                last_error = Some(e);
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

        match download_text(ALL_HTTPS_MIRRORS_URL).await {
            Ok(content) => {
                let uncommented = uncomment_servers(&content);
                if uncommented.contains("Server =") {
                    return Ok(uncommented);
                }
                last_error = Some(anyhow!("All-mirrors list contains no server entries"));
            }
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch all HTTPS mirrors")))
}

async fn validate_and_prioritize_mirrors(content: &str) -> Result<String> {
    let prepared = crate::common::pacman_mirrors::prepare_mirrorlist(
        content,
        crate::common::pacman_mirrors::DEFAULT_PROBE_LIMIT,
    )
    .await?;
    println!(
        "Selected working mirror after {} check(s): {} ({:.0} ms)",
        prepared.attempts,
        prepared.selected.mirror.template,
        prepared.selected.latency.as_secs_f64() * 1000.0
    );
    Ok(prepared.content)
}

async fn download_text(url: &str) -> Result<String> {
    reqwest::get(url)
        .await
        .with_context(|| format!("Failed to request {url}"))?
        .error_for_status()
        .with_context(|| format!("Mirror service returned an error for {url}"))?
        .text()
        .await
        .with_context(|| format!("Failed to read response from {url}"))
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
// Bundled Region Data (offline ISO)
// ============================================================================

/// Snapshot of archlinux.org region data staged into the offline bundle by
/// `instantOS/iso/offline/mk-region-data.sh`: the raw mirrorlist page plus
/// one raw per-region response under `mirrorlists/`.
fn bundled_regions_dir() -> PathBuf {
    Path::new(crate::arch::offline::BUNDLE_ROOT).join("regions")
}

/// name -> country-code from the bundle's `regions.html`; `None` when the
/// bundle carries no region snapshot (online ISO, partial/old bundle).
pub fn bundled_region_codes() -> Option<HashMap<String, String>> {
    load_region_codes(&bundled_regions_dir())
}

/// Read `regions.html` from `dir` with the same parser the live fetch uses.
fn load_region_codes(dir: &Path) -> Option<HashMap<String, String>> {
    let html = std::fs::read_to_string(dir.join("regions.html")).ok()?;
    let codes = parse_region_options(&html);
    (!codes.is_empty()).then_some(codes)
}

/// The bundled mirrorlist for a region selected in the wizard, put through
/// the same transform as the live fetch (uncomment + non-empty check) but
/// without network probing: the build-time API ordering is kept as-is.
pub fn bundled_region_mirrorlist(region_name: &str) -> Result<String> {
    load_region_mirrorlist(&bundled_regions_dir(), region_name)
}

fn load_region_mirrorlist(dir: &Path, region_name: &str) -> Result<String> {
    let codes = load_region_codes(dir).context("the offline bundle carries no region snapshot")?;
    let code = codes
        .get(region_name)
        .with_context(|| format!("region {region_name:?} is not in the bundle's region list"))?;
    let raw = std::fs::read_to_string(dir.join("mirrorlists").join(format!("{code}.txt")))
        .with_context(|| format!("the bundle has no mirrorlist for {region_name} ({code})"))?;
    let list = uncomment_servers(&raw);
    if !list.contains("Server =") {
        return Err(anyhow!(
            "the bundled mirrorlist for {region_name} contains no servers"
        ));
    }
    Ok(list)
}

// ============================================================================
// Data Provider
// ============================================================================

pub struct MirrorlistProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for MirrorlistProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        provide_mirrorlist(context, crate::arch::offline::mode()).await
    }
}

async fn provide_mirrorlist(
    context: &crate::arch::engine::InstallContext,
    mode: crate::arch::offline::Mode,
) -> Result<()> {
    if mode.is_offline() {
        provide_bundled_regions(context, bundled_region_codes());
        return Ok(());
    }
    provide_fetched_regions(context).await
}

/// Publish region data so the mirror-region question runs; shared by the
/// bundled and fetched paths.
fn publish_regions(
    context: &crate::arch::engine::InstallContext,
    regions: HashMap<String, String>,
) {
    let mut names: Vec<String> = regions.keys().cloned().collect();
    names.sort();
    context.set::<MirrorRegionsKey>(names);
    context.set::<MirrorRegionCodesKey>(regions);
    context.set::<MirrorRegionsFetchFailed>(false);
}

/// Offline: serve the build-time region snapshot so the question is still
/// asked and the choice shapes the installed system's mirrorlist. Without a
/// snapshot, degrade exactly like a failed fetch: empty list hides the
/// question and the fallback mirrorlist is used.
fn provide_bundled_regions(
    context: &crate::arch::engine::InstallContext,
    codes: Option<HashMap<String, String>>,
) {
    match codes {
        Some(codes) => {
            publish_regions(context, codes);
            println!(
                "Offline install: using the bundled region list; the selection shapes the installed system's mirrorlist."
            );
        }
        None => {
            println!("Offline install: bundle carries no region snapshot; selection skipped.");
            context.set::<MirrorRegionsKey>(Vec::new());
            context.set::<MirrorRegionsFetchFailed>(true);
        }
    }
}

async fn provide_fetched_regions(context: &crate::arch::engine::InstallContext) -> Result<()> {
    match fetch_mirror_regions().await {
        Ok(regions) => publish_regions(context, regions),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn offline_provider_skips_the_fetch_and_hides_the_question() {
        let context = crate::arch::engine::InstallContext::new();
        provide_mirrorlist(&context, crate::arch::offline::Mode::Strict)
            .await
            .unwrap();

        assert_eq!(context.get::<MirrorRegionsKey>(), Some(Vec::new()));
        assert_eq!(context.get::<MirrorRegionsFetchFailed>(), Some(true));
    }

    #[test]
    fn bundled_regions_publish_keys_and_show_the_question() {
        let context = crate::arch::engine::InstallContext::new();
        let mut codes = HashMap::new();
        codes.insert("Germany".to_string(), "de".to_string());
        codes.insert("Austria".to_string(), "at".to_string());

        provide_bundled_regions(&context, Some(codes));

        assert_eq!(
            context.get::<MirrorRegionsKey>(),
            Some(vec!["Austria".to_string(), "Germany".to_string()])
        );
        assert_eq!(
            context
                .get::<MirrorRegionCodesKey>()
                .and_then(|m| m.get("Germany").cloned()),
            Some("de".to_string())
        );
        assert_eq!(context.get::<MirrorRegionsFetchFailed>(), Some(false));
    }

    #[test]
    fn missing_bundled_regions_degrade_to_the_fetch_failed_skip() {
        let context = crate::arch::engine::InstallContext::new();
        provide_bundled_regions(&context, None);
        assert_eq!(context.get::<MirrorRegionsKey>(), Some(Vec::new()));
        assert_eq!(context.get::<MirrorRegionsFetchFailed>(), Some(true));
    }

    #[test]
    fn parse_region_options_reads_the_mirrorlist_page() {
        let html = "<option value=\"\">All</option>\n  <option value=\"de\">Germany</option>\n<option value=\"at\">Austria</option>\n";
        let codes = parse_region_options(html);
        assert_eq!(codes.get("Germany"), Some(&"de".to_string()));
        assert_eq!(codes.get("Austria"), Some(&"at".to_string()));
        assert_eq!(codes.len(), 2);
    }

    #[test]
    fn bundled_region_snapshot_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("regions.html"),
            "<option value=\"de\">Germany</option>\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("mirrorlists")).unwrap();
        std::fs::write(
            dir.path().join("mirrorlists").join("de.txt"),
            "#Server = https://mirror.example/$repo/os/$arch\n",
        )
        .unwrap();

        let codes = load_region_codes(dir.path()).expect("snapshot parses");
        assert_eq!(codes.get("Germany"), Some(&"de".to_string()));

        let list = load_region_mirrorlist(dir.path(), "Germany").unwrap();
        assert!(list.contains("Server = https://mirror.example/$repo/os/$arch"));

        assert!(load_region_mirrorlist(dir.path(), "Nowhere").is_err());
    }

    #[test]
    fn absent_region_snapshot_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_region_codes(dir.path()).is_none());
    }

    #[test]
    fn test_uncomment_servers_basic() {
        let input = "#Server = https://mirror.example.com/$repo/os/$arch\n## This is a comment\n";
        let result = uncomment_servers(input);
        assert!(result.contains("Server = https://mirror.example.com/$repo/os/$arch"));
        assert!(result.contains("## This is a comment"));
    }

    #[test]
    fn test_uncomment_servers_preserves_already_uncommented() {
        let input = "Server = https://mirror.example.com/$repo/os/$arch\n";
        let result = uncomment_servers(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_uncomment_servers_ignores_regular_comments() {
        let input = "## Arch Linux mirrorlist\n## Commented out by Pacman\n";
        let result = uncomment_servers(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_uncomment_servers_mixed_content() {
        let input = "\
## Arch Linux mirrorlist

## Germany
#Server = https://mirror.de/$repo/os/$arch
## France
#Server = https://mirror.fr/$repo/os/$arch
Server = https://already.active/$repo/os/$arch
";
        let result = uncomment_servers(input);
        assert!(result.contains("Server = https://mirror.de/$repo/os/$arch"));
        assert!(result.contains("Server = https://mirror.fr/$repo/os/$arch"));
        assert!(result.contains("Server = https://already.active/$repo/os/$arch"));
        assert!(result.contains("## Germany"));
        assert!(result.contains("## France"));
    }

    #[test]
    fn test_uncomment_servers_empty() {
        let result = uncomment_servers("");
        // empty string has no lines, so output is empty
        assert_eq!(result, "");
    }
}
