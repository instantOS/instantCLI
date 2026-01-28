//! AppImage management settings
//!
//! Launch Gear Lever to manage AppImages.

use anyhow::{Context, Result};
use std::process::{Command, Stdio};

use crate::settings::context::SettingsContext;
use crate::settings::deps::GEARLEVER;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;

pub struct ManageAppImages;

impl Setting for ManageAppImages {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.manage_appimages")
            .title("Manage AppImages")
            .icon(NerdFont::Package)
            .summary("Launch Gear Lever to manage and integrate AppImages.")
            .requirements(vec![&GEARLEVER])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn preview_command(&self) -> Option<String> {
        let script = PreviewBuilder::new()
            .header(NerdFont::Package, "Manage AppImages")
            .text("Launch Gear Lever to manage and integrate AppImages.")
            .separator()
            .field("Command", "flatpak run it.mijorus.gearlever")
            .build_shell_script();
        Some(script)
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Launching Gear Lever...");

        Command::new("flatpak")
            .args(["run", "it.mijorus.gearlever"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("launching Gear Lever")?;

        ctx.emit_success("settings.command.completed", "Launched Gear Lever");
        Ok(())
    }
}
