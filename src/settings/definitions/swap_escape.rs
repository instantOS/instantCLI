//! Swap Escape and Caps Lock setting

use anyhow::Result;

use crate::common::compositor::{CompositorType, sway};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Setting, SettingMetadata, SettingType};
use crate::settings::store::BoolSettingKey;
use crate::ui::prelude::*;

/// Swap Escape and Caps Lock keys
pub struct SwapEscape;

impl SwapEscape {
    const KEY: BoolSettingKey = BoolSettingKey::new("desktop.swap_escape", false);
}

impl Setting for SwapEscape {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("desktop.swap_escape")
            .title("Swap Escape and Caps Lock")
            .category(Category::Desktop)
            .icon(NerdFont::Keyboard)
            .breadcrumbs(&["Swap Escape and Caps Lock"])
            .summary("Swap the Escape and Caps Lock keys.\n\nWhen enabled, pressing Caps Lock will produce Escape and vice versa.\n\nSupports Sway and X11 window managers.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let target = !current;
        ctx.set_bool(Self::KEY, target);
        apply_swap_escape_impl(ctx, target)
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let enabled = ctx.bool(Self::KEY);
        if !enabled {
            return None;
        }
        Some(restore_swap_escape_impl(ctx))
    }
}

// Register at compile time
inventory::submit! {
    &SwapEscape as &'static dyn Setting
}

fn apply_swap_escape_impl(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();
    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_x11 = compositor.is_x11();

    if !is_sway && !is_x11 {
        ctx.emit_info(
            "settings.keyboard.swap_escape.unsupported",
            &format!(
                "Swap Escape/Caps Lock configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    if is_sway {
        let xkb_options = if enabled { "caps:swapescape" } else { "" };
        let cmd = format!("input type:keyboard xkb_options {}", xkb_options);
        match sway::swaymsg(&cmd) {
            Ok(_) => {
                ctx.notify(
                    "Swap Escape/Caps Lock",
                    if enabled {
                        "Escape and Caps Lock keys swapped"
                    } else {
                        "Escape and Caps Lock keys restored to normal"
                    },
                );
            }
            Err(e) => {
                ctx.emit_info(
                    "settings.keyboard.swap_escape.sway_failed",
                    &format!("Failed to apply in Sway: {e}"),
                );
            }
        }
    } else {
        let result = if enabled {
            std::process::Command::new("setxkbmap")
                .args(["-option", "caps:swapescape"])
                .status()
        } else {
            std::process::Command::new("setxkbmap")
                .args(["-option", ""])
                .status()
        };

        match result {
            Ok(status) if status.success() => {
                ctx.notify(
                    "Swap Escape/Caps Lock",
                    if enabled {
                        "Escape and Caps Lock keys swapped"
                    } else {
                        "Escape and Caps Lock keys restored to normal"
                    },
                );
            }
            Ok(_) => {
                ctx.emit_info(
                    "settings.keyboard.swap_escape.failed",
                    "setxkbmap command failed to apply the setting.",
                );
            }
            Err(e) => {
                ctx.emit_info(
                    "settings.keyboard.swap_escape.error",
                    &format!("Failed to execute setxkbmap: {e}"),
                );
            }
        }
    }

    Ok(())
}

fn restore_swap_escape_impl(_ctx: &mut SettingsContext) -> Result<()> {
    let compositor = CompositorType::detect();
    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_x11 = compositor.is_x11();

    if !is_sway && !is_x11 {
        return Ok(());
    }

    if is_sway {
        let cmd = "input type:keyboard xkb_options caps:swapescape";
        if let Err(e) = sway::swaymsg(cmd) {
            emit(
                Level::Warn,
                "settings.keyboard.swap_escape.restore_failed",
                &format!("Failed to restore swap escape setting in Sway: {e}"),
                None,
            );
        } else {
            emit(
                Level::Debug,
                "settings.keyboard.swap_escape.restored",
                "Restored swap escape setting in Sway",
                None,
            );
        }
    } else if let Err(e) = std::process::Command::new("setxkbmap")
        .args(["-option", "caps:swapescape"])
        .status()
    {
        emit(
            Level::Warn,
            "settings.keyboard.swap_escape.restore_failed",
            &format!("Failed to restore swap escape setting: {e}"),
            None,
        );
    } else {
        emit(
            Level::Debug,
            "settings.keyboard.swap_escape.restored",
            "Restored swap escape setting",
            None,
        );
    }

    Ok(())
}
