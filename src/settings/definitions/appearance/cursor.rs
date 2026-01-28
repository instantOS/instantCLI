//! Cursor theme settings
//!
//! Configure cursor themes for Sway.

use anyhow::Result;
use std::process::Command;

use crate::common::compositor::CompositorType;
use crate::menu_utils::FzfWrapper;
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

use super::common::list_cursor_themes;

pub struct CursorTheme;

impl Setting for CursorTheme {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.cursor_theme")
            .title("Cursor Theme")
            .icon(NerdFont::Mouse)
            .summary("Select and apply a cursor theme for Sway.\n\nUpdates gsettings cursor-theme setting.\nOnly supported on Sway.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::CursorTheme))
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::Sway) {
            ctx.emit_unsupported(
                "settings.appearance.cursor_theme.unsupported",
                &format!(
                    "Cursor theme configuration is only supported on Sway. Detected: {}",
                    compositor.name()
                ),
            );
            return Ok(());
        }

        let themes = list_cursor_themes()?;

        let mut options: Vec<String> = Vec::new();

        for theme in &themes {
            options.push(theme.clone());
        }

        if themes.is_empty() {
            ctx.emit_info(
                "settings.appearance.cursor_theme.no_themes",
                "No cursor themes found. Install cursor themes from your package manager.",
            );
            return Ok(());
        }

        let selected = FzfWrapper::builder()
            .prompt("Select Cursor Theme")
            .header("Choose a cursor theme to apply globally")
            .select(options)?;

        match selected {
            crate::menu_utils::FzfResult::Selected(selection) => {
                apply_cursor_theme_changes(ctx, &selection);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn apply_cursor_theme_changes(ctx: &mut SettingsContext, theme: &str) {
    // 1. Apply to GSettings first (for GTK apps) - setup_sway reads from gsettings
    let status = Command::new("timeout")
        .args([
            "10s",
            "gsettings",
            "set",
            "org.gnome.desktop.interface",
            "cursor-theme",
            theme,
        ])
        .status();

    match status {
        Ok(exit) if exit.success() => {
            ctx.notify("Cursor Theme", &format!("Applied '{}' to GSettings", theme));
        }
        Ok(exit) => {
            ctx.emit_failure(
                "settings.appearance.cursor_theme.gsettings_failed",
                &format!(
                    "GSettings failed with exit code {}",
                    exit.code().unwrap_or(-1)
                ),
            );
        }
        Err(e) => {
            ctx.emit_failure(
                "settings.appearance.cursor_theme.gsettings_error",
                &format!("Failed to execute gsettings: {e}"),
            );
        }
    }

    // 2. Regenerate sway config (reads cursor theme from gsettings)
    if let Err(e) = crate::setup::setup_sway() {
        ctx.emit_failure(
            "settings.appearance.cursor_theme.sway_config_error",
            &format!("Failed to update sway config: {e}"),
        );
    }
}
