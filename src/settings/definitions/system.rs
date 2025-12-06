//! System settings
//!
//! System administration, updates, and firmware settings.

use anyhow::{Context, Result};
use duct::cmd;

use crate::common::requirements::{
    COCKPIT_PACKAGE, FASTFETCH_PACKAGE, GNOME_FIRMWARE_PACKAGE, TOPGRADE_PACKAGE,
};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// About System (uses shell command with read, can't use macro)
// ============================================================================

pub struct AboutSystem;

impl Setting for AboutSystem {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "system.about",
            title: "About",
            category: Category::System,
            icon: NerdFont::About,
            breadcrumbs: &["About"],
            summary: "Display system information using fastfetch.",
            requires_reapply: false,
            requirements: &[Requirement::Package(FASTFETCH_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info(
            "settings.command.launching",
            "Displaying system information...",
        );
        cmd!("sh", "-c", "fastfetch && read -n 1")
            .run()
            .context("running fastfetch")?;
        Ok(())
    }
}

inventory::submit! { &AboutSystem as &'static dyn Setting }

// ============================================================================
// Cockpit (uses custom launch logic, can't use macro)
// ============================================================================

pub struct CockpitManager;

impl Setting for CockpitManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "system.cockpit",
            title: "Systemd manager (Cockpit)",
            category: Category::System,
            icon: NerdFont::Server,
            breadcrumbs: &["Systemd manager"],
            summary: "Launch Cockpit web interface for managing systemd services, logs, and system resources.",
            requires_reapply: false,
            requirements: &[Requirement::Package(COCKPIT_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        crate::settings::actions::launch_cockpit(ctx)
    }
}

inventory::submit! { &CockpitManager as &'static dyn Setting }

// ============================================================================
// Firmware Manager (GUI app)
// ============================================================================

gui_command_setting!(
    FirmwareManager,
    "system.firmware",
    "Firmware Manager",
    Category::System,
    NerdFont::Cpu,
    "Launch GNOME Firmware manager to view and update device firmware.\n\nManage firmware for BIOS/UEFI, devices, and peripherals.",
    "gnome-firmware",
    GNOME_FIRMWARE_PACKAGE
);

// ============================================================================
// System Upgrade (TUI app)
// ============================================================================

tui_command_setting!(
    SystemUpgrade,
    "system.upgrade",
    "Upgrade",
    Category::System,
    NerdFont::Upgrade,
    "Upgrade all installed packages and system components using topgrade.",
    "topgrade",
    TOPGRADE_PACKAGE
);
