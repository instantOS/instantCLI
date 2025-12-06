//! System settings
//!
//! System administration, updates, and firmware settings.

use anyhow::{Context, Result};
use duct::cmd;
use std::process::Command;

use crate::common::requirements::{FASTFETCH_PACKAGE, GNOME_FIRMWARE_PACKAGE, TOPGRADE_PACKAGE, COCKPIT_PACKAGE};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// About System
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
        ctx.emit_info("settings.command.launching", "Displaying system information...");
        cmd!("sh", "-c", "fastfetch && read -n 1").run().context("running fastfetch")?;
        Ok(())
    }
}

inventory::submit! { &AboutSystem as &'static dyn Setting }

// ============================================================================
// Cockpit (Systemd Manager)
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
// Firmware Manager
// ============================================================================

pub struct FirmwareManager;

impl Setting for FirmwareManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "system.firmware",
            title: "Firmware Manager",
            category: Category::System,
            icon: NerdFont::Cpu,
            breadcrumbs: &["Firmware Manager"],
            summary: "Launch GNOME Firmware manager to view and update device firmware.\n\nManage firmware for BIOS/UEFI, devices, and peripherals.",
            requires_reapply: false,
            requirements: &[Requirement::Package(GNOME_FIRMWARE_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Launching GNOME Firmware...");
        Command::new("gnome-firmware").spawn().context("launching gnome-firmware")?;
        ctx.emit_success("settings.command.completed", "Launched Firmware Manager");
        Ok(())
    }
}

inventory::submit! { &FirmwareManager as &'static dyn Setting }

// ============================================================================
// System Upgrade
// ============================================================================

pub struct SystemUpgrade;

impl Setting for SystemUpgrade {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "system.upgrade",
            title: "Upgrade",
            category: Category::System,
            icon: NerdFont::Upgrade,
            breadcrumbs: &["Upgrade"],
            summary: "Upgrade all installed packages and system components using topgrade.",
            requires_reapply: false,
            requirements: &[Requirement::Package(TOPGRADE_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Running system upgrade with topgrade...");
        cmd!("topgrade").run().context("running topgrade")?;
        ctx.emit_success("settings.command.completed", "System upgrade completed");
        Ok(())
    }
}

inventory::submit! { &SystemUpgrade as &'static dyn Setting }
