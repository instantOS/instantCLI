//! Connectivity required by the installation source, rather than ICMP reachability.
use std::time::Duration;

use anyhow::Result;

pub async fn available() -> Result<bool> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()?;
    Ok(client
        .get("https://archlinux.org/releng/releases/json/")
        .send()
        .await
        .is_ok_and(|response| response.status().is_success()))
}
