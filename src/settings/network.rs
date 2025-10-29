use anyhow::{Context, Result};
use std::process::Command;

use crate::common::requirements::RequiredPackage;
use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;

use super::context::SettingsContext;

pub const NM_CONNECTION_EDITOR_PACKAGE: RequiredPackage = RequiredPackage {
    name: "NetworkManager connection editor",
    arch_package_name: Some("nm-connection-editor"),
    ubuntu_package_name: Some("network-manager-gnome"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "nm-connection-editor",
    )],
};

pub const CHROMIUM_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Chromium browser",
    arch_package_name: Some("chromium"),
    ubuntu_package_name: Some("chromium-browser"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "chromium",
    )],
};

/// Show IP address information
pub fn show_ip_info(ctx: &mut SettingsContext) -> Result<()> {
    ctx.emit_info("settings.network.ip_info", "Gathering network information...");

    // Get local IP address
    let local_ip = get_local_ip();

    // Get public IP address
    let public_ip = if check_internet() {
        get_public_ip().ok()
    } else {
        None
    };

    // Build the message
    let mut message = String::new();
    message.push_str("═══════════════════════════════════════\n");
    message.push_str("         Network Information\n");
    message.push_str("═══════════════════════════════════════\n\n");

    if let Some(ref local) = local_ip {
        message.push_str(&format!("{}  Local IP:  {}\n", char::from(NerdFont::Desktop), local));
    } else {
        message.push_str(&format!("{}  Local IP:  Not found\n", char::from(NerdFont::Warning)));
    }

    if let Some(ref public) = public_ip {
        message.push_str(&format!("{}  Public IP: {}\n", char::from(NerdFont::Globe), public));
    } else {
        message.push_str(&format!("{}  Public IP: Not found\n", char::from(NerdFont::Warning)));
    }

    message.push_str("\n");

    if local_ip.is_none() && public_ip.is_none() {
        message.push_str(&format!("{} No network connection detected\n", char::from(NerdFont::CrossCircle)));
    }

    // Show the message
    FzfWrapper::builder()
        .title("Network Information")
        .message(message)
        .show_message()?;

    ctx.emit_success("settings.network.ip_info.shown", "Network information displayed");

    Ok(())
}

/// Get local IP address
fn get_local_ip() -> Option<String> {
    // Try using `ip` command first (more modern)
    if let Ok(output) = Command::new("ip")
        .args(&["route", "get", "1.1.1.1"])
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
fn get_public_ip() -> Result<String> {
    let output = Command::new("curl")
        .args(&["-s", "--max-time", "5", "ifconfig.me"])
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
fn check_internet() -> bool {
    Command::new("ping")
        .args(&["-c", "1", "-W", "2", "1.1.1.1"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Launch speed test using fast.com
pub fn launch_speed_test(ctx: &mut SettingsContext) -> Result<()> {
    ctx.emit_info("settings.network.speedtest", "Launching speed test...");

    // Launch chromium in app mode to fast.com
    Command::new("chromium")
        .arg("--app=https://fast.com")
        .spawn()
        .context("Failed to launch chromium")?;

    ctx.emit_success(
        "settings.network.speedtest.launched",
        "Opened fast.com speed test",
    );

    Ok(())
}

/// Launch NetworkManager connection editor
pub fn edit_connections(ctx: &mut SettingsContext) -> Result<()> {
    ctx.emit_info(
        "settings.network.edit_connections",
        "Launching connection editor...",
    );

    match Command::new("nm-connection-editor").status() {
        Ok(status) if status.success() => {
            ctx.emit_success(
                "settings.network.edit_connections.success",
                "Connection editor closed",
            );
        }
        Ok(status) => {
            emit(
                Level::Warn,
                "settings.network.edit_connections.exit_status",
                &format!(
                    "{} nm-connection-editor exited with status {:?}",
                    char::from(NerdFont::Warning),
                    status.code()
                ),
                None,
            );
        }
        Err(err) => anyhow::bail!("Failed to start nm-connection-editor: {err}"),
    }

    Ok(())
}

