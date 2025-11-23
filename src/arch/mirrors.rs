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
        if line.starts_with("<option value=\"") {
            if let Some(start_quote) = line.find('"') {
                if let Some(end_quote) = line[start_quote + 1..].find('"') {
                    let code = &line[start_quote + 1..start_quote + 1 + end_quote];

                    if let Some(close_tag) = line.find('>') {
                        if let Some(end_tag) = line.find("</option>") {
                            let name = &line[close_tag + 1..end_tag];

                            if !code.is_empty() && !name.is_empty() && name != "All" {
                                regions.insert(name.to_string(), code.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(regions)
}

pub struct MirrorlistProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for MirrorlistProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        match fetch_mirror_regions().await {
            Ok(regions) => {
                let mut data = context.data.lock().unwrap();
                let mut names: Vec<String> = regions.keys().cloned().collect();
                names.sort();
                data.insert("mirror_regions".to_string(), names.join(","));
                if let Ok(json) = serde_json::to_string(&regions) {
                    data.insert("mirror_map".to_string(), json);
                }
            }
            Err(e) => {
                eprintln!("Failed to fetch mirror regions: {}", e);
                let mut data = context.data.lock().unwrap();
                data.insert("mirror_regions".to_string(), "Worldwide".to_string());
            }
        }
        Ok(())
    }
}
