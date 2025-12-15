//! Swap Escape and Caps Lock setting

use anyhow::Result;

use crate::common::compositor::{CompositorType, sway};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::BoolSettingKey;
use crate::ui::prelude::*;

/// Swap Escape and Caps Lock keys
pub struct SwapEscape;

impl SwapEscape {
    const KEY: BoolSettingKey = BoolSettingKey::new("desktop.swap_escape", false);
}

/// Apply swap escape setting (shared by both apply and restore)
fn apply_swap_escape_setting(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();
    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_gnome = matches!(compositor, CompositorType::Gnome);
    let is_x11 = compositor.is_x11();

    if !is_sway && !is_gnome && !is_x11 {
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
    } else if is_gnome {
        let result = if enabled {
            std::process::Command::new("gsettings")
                .args([
                    "set",
                    "org.gnome.desktop.input-sources",
                    "xkb-options",
                    "['caps:swapescape']",
                ])
                .status()
        } else {
            std::process::Command::new("gsettings")
                .args(["reset", "org.gnome.desktop.input-sources", "xkb-options"])
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
                    "gsettings command failed to apply the setting.",
                );
            }
            Err(e) => {
                ctx.emit_info(
                    "settings.keyboard.swap_escape.error",
                    &format!("Failed to execute gsettings: {e}"),
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

impl Setting for SwapEscape {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("desktop.swap_escape")
            .title("Swap Escape and Caps Lock")
            .icon(NerdFont::Keyboard)
            .summary("Swap the Escape and Caps Lock keys.\n\nWhen enabled, pressing Caps Lock will produce Escape and vice versa.\n\nSupports Sway, GNOME, and X11 window managers.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);
        self.apply_value(ctx, enabled)
    }

    fn apply_value(&self, ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
        apply_swap_escape_setting(ctx, enabled)
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let enabled = ctx.bool(Self::KEY);
        if !enabled {
            return None;
        }
        Some(self.apply_value(ctx, enabled))
    }
}
