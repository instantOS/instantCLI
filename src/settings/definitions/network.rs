//! Network settings
//!
//! IP info, speed test, and connection management.

use anyhow::{Context, Result};
use duct::cmd;
use std::process::{Command, Stdio};

use crate::settings::context::SettingsContext;
use crate::settings::deps::{CHROMIUM, NM_CONNECTION_EDITOR, NMTUI};
use crate::settings::network;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
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
            .requirements(vec![&CHROMIUM])
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
            .title("Edit Connection (Advanced)")
            .icon(NerdFont::Sliders)
            .summary("Manage WiFi, Ethernet, VPN, and other network connections.\n\nConfigure connection settings, passwords, and advanced options.")
            .requirements(vec![&NM_CONNECTION_EDITOR])
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

// ============================================================================
// Edit Connections (TUI app)
// ============================================================================

pub struct EditConnectionsTui;

impl Setting for EditConnectionsTui {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("network.edit_connections_tui")
            .title("Edit Connections")
            .icon(NerdFont::Network)
            .summary("Manage network connections using the terminal interface.")
            .requirements(vec![&NMTUI])
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

        // nmtui (via libnewt) only supports standard ANSI color names.
        // We use a "soft mono" theme: Light Gray text on Black background with Blue accents.
        // This avoids the harshness of bright white and the oversaturation of default blue,
        // while blending better with dark terminal themes like Catppuccin.
        let newt_colors = concat!(
            "root=lightgray,black ",
            "border=blue,black ",
            "window=lightgray,black ",
            "shadow=black,black ",
            "title=blue,black ",
            "button=black,lightgray ",
            "actbutton=black,blue ",
            "checkbox=blue,black ",
            "actcheckbox=black,blue ",
            "entry=lightgray,black ",
            "label=lightgray,black ",
            "listbox=lightgray,black ",
            "actlistbox=black,blue ",
            "sellistbox=lightgray,black ",
            "actsellistbox=black,blue ",
            "textbox=lightgray,black ",
            "acttextbox=lightgray,black ",
            "helpline=lightgray,black ",
            "roottext=lightgray,black ",
            "emptyscale=black,black ",
            "fullscale=blue,blue ",
            "disentry=gray,black ",
            "compactbutton=black,lightgray"
        );

        cmd!("nmtui")
            .env("NEWT_COLORS", newt_colors)
            .run()
            .context("running nmtui")?;
        ctx.emit_success("settings.command.completed", "Exited Edit Connections");
        Ok(())
    }
}
