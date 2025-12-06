//! Network settings
//!
//! IP info, speed test, and connection management.

use anyhow::{Context, Result};
use std::process::{Command, Stdio};

use crate::common::requirements::{CHROMIUM_PACKAGE, NM_CONNECTION_EDITOR_PACKAGE};
use crate::settings::context::SettingsContext;
use crate::settings::network;
use crate::settings::setting::{Requirement, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// IP Address Info (custom logic, can't use macro)
// ============================================================================

pub struct IpInfo;

impl Setting for IpInfo {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("network.ip_info")
            .title("IP Address Info")
            .icon(NerdFont::Info)
            .summary("View your local and public IP addresses.\n\nUseful for troubleshooting network issues or setting up remote access.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        network::show_ip_info(ctx)
    }
}

// ============================================================================
// Internet Speed Test (needs args, can't use simple macro)
// ============================================================================

pub struct SpeedTest;

impl Setting for SpeedTest {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("network.speed_test")
            .title("Internet Speed Test")
            .icon(NerdFont::Rocket)
            .summary("Test your internet connection speed using fast.com.\n\nMeasures download speed from Netflix servers.")
            .requirements(&[Requirement::Package(CHROMIUM_PACKAGE)])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info(
            "settings.command.launching",
            "Opening fast.com in Chromium...",
        );
        Command::new("chromium")
            .args(["--app=https://fast.com"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("launching chromium")?;
        ctx.emit_success("settings.command.completed", "Launched speed test");
        Ok(())
    }
}

// ============================================================================
// Edit Connections (GUI app)
// ============================================================================

pub struct EditConnections;

impl Setting for EditConnections {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("network.edit_connections")
            .title("Edit Connections")
            .icon(NerdFont::Settings)
            .summary("Manage WiFi, Ethernet, VPN, and other network connections.\n\nConfigure connection settings, passwords, and advanced options.")
            .requirements(&[Requirement::Package(NM_CONNECTION_EDITOR_PACKAGE)])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info(
            "settings.command.launching",
            "Launching Edit Connections...",
        );
        Command::new("nm-connection-editor")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("launching nm-connection-editor")?;
        ctx.emit_success("settings.command.completed", "Launched Edit Connections");
        Ok(())
    }
}
