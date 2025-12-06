//! Network settings
//!
//! IP info, speed test, and connection management.

use anyhow::{Context, Result};
use std::process::{Command, Stdio};

use crate::common::requirements::{CHROMIUM_PACKAGE, NM_CONNECTION_EDITOR_PACKAGE};
use crate::settings::context::SettingsContext;
use crate::settings::network;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// IP Address Info (custom logic, can't use macro)
// ============================================================================

pub struct IpInfo;

impl Setting for IpInfo {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "network.ip_info",
            title: "IP Address Info",
            category: Category::Network,
            icon: NerdFont::Info,
            icon_color: None,
            breadcrumbs: &["IP Address Info"],
            summary: "View your local and public IP addresses.\n\nUseful for troubleshooting network issues or setting up remote access.",
            requires_reapply: false,
            requirements: &[],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        network::show_ip_info(ctx)
    }
}

inventory::submit! { &IpInfo as &'static dyn Setting }

// ============================================================================
// Internet Speed Test (needs args, can't use simple macro)
// ============================================================================

pub struct SpeedTest;

impl Setting for SpeedTest {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "network.speed_test",
            title: "Internet Speed Test",
            category: Category::Network,
            icon: NerdFont::Rocket,
            icon_color: None,
            breadcrumbs: &["Internet Speed Test"],
            summary: "Test your internet connection speed using fast.com.\n\nMeasures download speed from Netflix servers.",
            requires_reapply: false,
            requirements: &[Requirement::Package(CHROMIUM_PACKAGE)],
        }
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

inventory::submit! { &SpeedTest as &'static dyn Setting }

// ============================================================================
// Edit Connections (GUI app)
// ============================================================================

gui_command_setting!(
    EditConnections,
    "network.edit_connections",
    "Edit Connections",
    Category::Network,
    NerdFont::Settings,
    "Manage WiFi, Ethernet, VPN, and other network connections.\n\nConfigure connection settings, passwords, and advanced options.",
    "nm-connection-editor",
    NM_CONNECTION_EDITOR_PACKAGE
);
