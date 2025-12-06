//! Storage settings (additional)
//!
//! Disk management and partition editor.

use anyhow::{Context, Result};
use std::process::Command;

use crate::common::requirements::{GNOME_DISKS_PACKAGE, GPARTED_PACKAGE};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// GNOME Disks
// ============================================================================

pub struct DiskManagement;

impl Setting for DiskManagement {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "storage.disks",
            title: "Disk management",
            category: Category::Storage,
            icon: NerdFont::HardDrive,
            breadcrumbs: &["Disk management"],
            summary: "Launch GNOME Disks to manage drives and partitions.",
            requires_reapply: false,
            requirements: &[Requirement::Package(GNOME_DISKS_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Launching GNOME Disks...");
        Command::new("gnome-disks")
            .spawn()
            .context("launching gnome-disks")?;
        ctx.emit_success("settings.command.completed", "Launched GNOME Disks");
        Ok(())
    }
}

inventory::submit! { &DiskManagement as &'static dyn Setting }

// ============================================================================
// GParted
// ============================================================================

pub struct PartitionEditor;

impl Setting for PartitionEditor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "storage.gparted",
            title: "Partition editor",
            category: Category::Storage,
            icon: NerdFont::Partition,
            breadcrumbs: &["Partition editor"],
            summary: "Launch GParted for advanced partition management (requires root).",
            requires_reapply: false,
            requirements: &[Requirement::Package(GPARTED_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Launching GParted...");
        Command::new("gparted")
            .spawn()
            .context("launching gparted")?;
        ctx.emit_success("settings.command.completed", "Launched GParted");
        Ok(())
    }
}

inventory::submit! { &PartitionEditor as &'static dyn Setting }
