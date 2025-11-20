use anyhow::{Context, Result};
use std::process::Command;

use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;

use super::context::SettingsContext;

/// Show IP address information
pub fn show_ip_info(ctx: &mut SettingsContext) -> Result<()> {
    ctx.emit_info(
        "settings.network.ip_info",
        "Gathering network information...",
    );

    // Get local IP address
    let local_ip = get_local_ip();

    // Check internet connectivity
    let has_internet = check_internet();

    // Get public IP address (only if internet is available)
    let public_ip = if has_internet {
        get_public_ip().ok()
    } else {
        None
    };

    // Build the message
    let mut message = String::new();
    message.push_str("═══════════════════════════════════════\n");
    message.push_str("         Network Information\n");
    message.push_str("═══════════════════════════════════════\n\n");

    // Internet status
    if has_internet {
        message.push_str(&format!(
            "{}  Internet:  Connected\n",
            char::from(NerdFont::CheckCircle)
        ));
    } else {
        message.push_str(&format!(
            "{}  Internet:  Not connected\n",
            char::from(NerdFont::CrossCircle)
        ));
    }

    message.push('\n');

    // Local IP
    if let Some(ref local) = local_ip {
        message.push_str(&format!(
            "{}  Local IP:  {}\n",
            char::from(NerdFont::Desktop),
            local
        ));
    } else {
        message.push_str(&format!(
            "{}  Local IP:  Not found\n",
            char::from(NerdFont::Warning)
        ));
    }

    // Public IP
    if let Some(ref public) = public_ip {
        message.push_str(&format!(
            "{}  Public IP: {}\n",
            char::from(NerdFont::Globe),
            public
        ));
    } else if has_internet {
        message.push_str(&format!(
            "{}  Public IP: Unable to retrieve\n",
            char::from(NerdFont::Warning)
        ));
    } else {
        message.push_str(&format!(
            "{}  Public IP: Not available (no internet)\n",
            char::from(NerdFont::Warning)
        ));
    }

    message.push('\n');

    // Additional status message
    if local_ip.is_none() && !has_internet {
        message.push_str(&format!(
            "{} No network connection detected\n",
            char::from(NerdFont::CrossCircle)
        ));
    } else if local_ip.is_some() && !has_internet {
        message.push_str(&format!(
            "{} Local network only (no internet access)\n",
            char::from(NerdFont::Info)
        ));
    }

    // Show the message
    FzfWrapper::builder()
        .title("Network Information")
        .message(message)
        .show_message()?;

    ctx.emit_success(
        "settings.network.ip_info.shown",
        "Network information displayed",
    );

    Ok(())
}

/// Get local IP address
fn get_local_ip() -> Option<String> {
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
fn get_public_ip() -> Result<String> {
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
