use anyhow::{Context, Result};
use std::process::Command;

/// Get the default network interface
pub fn get_default_interface() -> Option<String> {
    if let Ok(output) = Command::new("ip")
        .args(["route", "get", "1.1.1.1"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse output like: "1.1.1.1 via 192.168.1.1 dev wlan0 src 192.168.1.100"
            for line in stdout.lines() {
                if let Some(dev_pos) = line.find(" dev ") {
                    let after_dev = &line[dev_pos + 5..];
                    if let Some(dev) = after_dev.split_whitespace().next() {
                        return Some(dev.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Get IP address for a specific interface
pub fn get_ip_from_interface(interface: &str) -> Option<String> {
    if let Ok(output) = Command::new("ip")
        .args(["-4", "addr", "show", interface])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse output looking for "inet <ip> ..."
            for line in stdout.lines() {
                let line = line.trim();
                if line.starts_with("inet ") {
                    // "inet 192.168.1.100/24 ..."
                    if let Some(ip_cidr) = line.split_whitespace().nth(1) {
                        // Extract IP from IP/CIDR (e.g., 192.168.1.100 from 192.168.1.100/24)
                        if let Some(ip) = ip_cidr.split('/').next() {
                            return Some(ip.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Get local IP address
///
/// If interface is provided, tries to get IP for that interface.
/// Otherwise, tries to detect default interface and IP.
pub fn get_local_ip(interface: Option<&str>) -> Option<String> {
    // If interface is specified, use it
    if let Some(iface) = interface {
        return get_ip_from_interface(iface);
    }

    // Try using `ip` command to find default route source
    if let Ok(output) = Command::new("ip")
        .args(["route", "get", "1.1.1.1"])
        .output()
    {
        if output.status.success() {
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
    }

    // Fallback to hostname -I
    if let Ok(output) = Command::new("hostname").arg("-I").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(ip) = stdout.split_whitespace().next() {
                return Some(ip.to_string());
            }
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
