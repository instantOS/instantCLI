use anyhow::Result;
use std::collections::HashMap;

pub async fn fetch_mirror_regions() -> Result<HashMap<String, String>> {
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

pub async fn fetch_mirrorlist(region_code: &str) -> Result<String> {
    let url = format!(
        "https://archlinux.org/mirrorlist/?country={}&protocol=https&ip_version=4",
        region_code
    );
    let response = reqwest::get(&url).await?.text().await?;

    // Uncomment servers
    let mut mirrorlist = String::new();
    for line in response.lines() {
        if line.starts_with("#Server =") {
            mirrorlist.push_str(&line[1..]); // Remove #
        } else {
            mirrorlist.push_str(line);
        }
        mirrorlist.push('\n');
    }

    Ok(mirrorlist)
}

use crate::arch::engine::DataKey;

pub struct MirrorRegionsKey;

impl DataKey for MirrorRegionsKey {
    type Value = Vec<String>;
    const KEY: &'static str = "mirror_regions";
}

pub struct MirrorlistProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for MirrorlistProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        match fetch_mirror_regions().await {
            Ok(regions) => {
                let mut names: Vec<String> = regions.keys().cloned().collect();
                names.sort();
                context.set::<MirrorRegionsKey>(names);
                // Note: We are not storing the full map in context anymore as it wasn't used by questions directly
                // If needed, we can define another key for it.
            }
            Err(e) => {
                eprintln!("Failed to fetch mirror regions: {}", e);
                context.set::<MirrorRegionsKey>(vec!["Worldwide".to_string()]);
            }
        }
        Ok(())
    }
}
