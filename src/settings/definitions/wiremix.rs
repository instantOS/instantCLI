//! Launch wiremix audio settings

use anyhow::{Context, Result};
use duct::cmd;

use crate::common::requirements::WIREMIX_PACKAGE;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

/// Launch wiremix for audio settings
pub struct LaunchWiremix;

impl Setting for LaunchWiremix {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "audio.wiremix",
            title: "General audio settings",
            category: Category::Audio,
            icon: NerdFont::Settings,
            breadcrumbs: &["General audio settings"],
            summary: "Launch wiremix TUI to manage PipeWire routing and volumes.",
            requires_reapply: false,
            requirements: &[Requirement::Package(WIREMIX_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info(
            "settings.command.launching",
            "Launch wiremix TUI to manage PipeWire routing and volumes.",
        );

        cmd!("wiremix")
            .run()
            .context("running wiremix")?;

        ctx.emit_success(
            "settings.command.completed",
            "Launched General audio settings",
        );

        Ok(())
    }

    // No restore needed - commands don't persist state
}

// Register at compile time
inventory::submit! {
    &LaunchWiremix as &'static dyn Setting
}
