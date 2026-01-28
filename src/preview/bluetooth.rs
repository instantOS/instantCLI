use std::process::Command;

use anyhow::Result;

use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_bluetooth_preview() -> Result<String> {
    let green = hex_to_ansi_fg(colors::GREEN);
    let red = hex_to_ansi_fg(colors::RED);
    let teal = hex_to_ansi_fg(colors::TEAL);
    let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
    let mauve = hex_to_ansi_fg(colors::MAUVE);
    let reset = "\x1b[0m";

    let systemd = crate::common::systemd::SystemdManager::system();
    let active = systemd.is_active("bluetooth.service");
    let enabled = systemd.is_enabled("bluetooth.service");

    let mut builder = PreviewBuilder::new()
        .line(colors::MAUVE, Some(NerdFont::Bluetooth), "Bluetooth")
        .separator()
        .blank()
        .text("Turn Bluetooth on or off.")
        .blank()
        .line(colors::TEAL, Some(NerdFont::ChevronRight), "Features")
        .bullets([
            "Connect wireless headphones & speakers",
            "Pair keyboards and mice",
            "Transfer files between devices",
        ])
        .blank()
        .line(colors::TEAL, Some(NerdFont::ChevronRight), "Current Status");

    if active {
        let icon = char::from(NerdFont::CircleCheck);
        builder = builder.raw(&format!(
            "  {green}{icon}{reset} Bluetooth service is running"
        ));
    } else {
        let icon = char::from(NerdFont::Circle);
        builder = builder.raw(&format!(
            "  {red}{icon}{reset} Bluetooth service is stopped"
        ));
    }

    if enabled {
        builder = builder.raw(&format!("  {teal}  Enabled at boot{reset}"));
    } else {
        builder = builder.raw(&format!("  {subtext}  Disabled at boot{reset}"));
    }

    builder = builder.blank().line(
        colors::TEAL,
        Some(NerdFont::ChevronRight),
        "Connected Devices",
    );

    if which::which("bluetoothctl").is_err() {
        builder = builder.raw(&format!("  {subtext}bluetoothctl not installed{reset}"));
        return Ok(builder.build_string());
    }

    let devices = bluetooth_connected_devices();
    if devices.is_empty() {
        builder = builder.raw(&format!("  {subtext}No devices connected{reset}"));
    } else {
        for device in devices {
            let bullet = char::from(NerdFont::Bullet);
            builder = builder.raw(&format!("  {mauve}{bullet}{reset} {device}"));
        }
    }

    Ok(builder.build_string())
}

fn bluetooth_connected_devices() -> Vec<String> {
    let output = Command::new("bluetoothctl")
        .args(["devices", "Connected"])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Device ") {
                let mut parts = rest.splitn(2, ' ');
                let _mac = parts.next();
                let name = parts.next().unwrap_or("").trim();
                if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                }
            } else {
                None
            }
        })
        .collect()
}
