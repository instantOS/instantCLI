use std::process::Command;

use anyhow::Result;

use crate::ui::catppuccin::colors;
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_bluetooth_preview() -> Result<String> {
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

    builder = if active {
        builder.indented_line(
            colors::GREEN,
            Some(NerdFont::CircleCheck),
            "Bluetooth service is running",
        )
    } else {
        builder.indented_line(
            colors::RED,
            Some(NerdFont::Circle),
            "Bluetooth service is stopped",
        )
    };
    builder = builder.indented_line(
        if enabled {
            colors::TEAL
        } else {
            colors::SUBTEXT0
        },
        None,
        if enabled {
            "Enabled at boot"
        } else {
            "Disabled at boot"
        },
    );

    builder = builder.blank().line(
        colors::TEAL,
        Some(NerdFont::ChevronRight),
        "Connected Devices",
    );

    if which::which("bluetoothctl").is_err() {
        builder = builder.subtext("bluetoothctl not installed");
        return Ok(builder.build_string());
    }

    let devices = bluetooth_connected_devices();
    if devices.is_empty() {
        builder = builder.subtext("No devices connected");
    } else {
        for device in devices {
            builder = builder.bullet(&device);
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
