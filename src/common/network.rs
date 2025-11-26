use anyhow::{Context, Result};
use std::process::Command;

/// Get local IP address
pub fn get_local_ip() -> Option<String> {
    // Try using `ip` command first (more modern)
    if let Ok(output) = Command::new("ip")
        .args(["route", "get", "1.1.1.1"])
        .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse output like: "1.1.1.1 via 192.168.1.1 dev wlan0 src 192.168.1.100"
        for line in stdout.lines() {
            if let Some(src_pos) = line.find(" src ") {
                let after_src = &line[src_pos + 5..];
                if let Some(ip) = after_src.split_whitespace().next() {
                    return Some(ip.to_string());
                }
            }
        }
    }

    // Fallback to hostname -I
    if let Ok(output) = Command::new("hostname").arg("-I").output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(ip) = stdout.split_whitespace().next() {
            return Some(ip.to_string());
        }
    }

    None
}

/// Get public IP address
pub fn get_public_ip() -> Result<String> {
    let output = Command::new("curl")
        .args(["-s", "--max-time", "5", "ifconfig.me"])
        .output()
        .context("Failed to execute curl")?;

    if !output.status.success() {
        anyhow::bail!("curl command failed");
    }

    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ip.is_empty() {
        anyhow::bail!("No public IP returned");
    }

    Ok(ip)
}

/// Check if internet is available
pub fn check_internet() -> bool {
    Command::new("ping")
        .args(["-c", "1", "-W", "2", "1.1.1.1"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
