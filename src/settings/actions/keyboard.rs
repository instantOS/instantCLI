//! Keyboard-related settings actions

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::common::compositor::{CompositorType, sway};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::super::context::SettingsContext;

#[derive(Clone)]
struct LayoutChoice {
    code: String,
    name: String,
}

impl FzfSelectable for LayoutChoice {
    fn fzf_display_text(&self) -> String {
        format!("{} ({})", self.name, self.code)
    }

    fn fzf_key(&self) -> String {
        self.code.clone()
    }
}

fn parse_xkb_layouts() -> Result<Vec<LayoutChoice>> {
    let file = File::open("/usr/share/X11/xkb/rules/evdev.lst")
        .context("Failed to open /usr/share/X11/xkb/rules/evdev.lst")?;
    let reader = BufReader::new(file);

    let mut layouts = Vec::new();
    let mut in_layout_section = false;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed == "! layout" {
            in_layout_section = true;
            continue;
        }

        if trimmed == "! variant" {
            break;
        }

        if in_layout_section && !trimmed.starts_with('!') && !trimmed.is_empty() {
            // Parse line: "code   description"
            let parts: Vec<&str> = trimmed.splitn(2, |c: char| c.is_whitespace()).collect();
            if parts.len() == 2 {
                let code = parts[0].trim().to_string();
                let name = parts[1].trim().to_string();
                layouts.push(LayoutChoice { code, name });
            }
        }
    }

    Ok(layouts)
}

pub fn configure_keyboard_layout(ctx: &mut SettingsContext) -> Result<()> {
    let compositor = CompositorType::detect();

    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_x11 = compositor.is_x11();

    // Check support
    if !is_sway && !is_x11 {
        ctx.emit_info(
            "settings.keyboard.unsupported",
            "Keyboard layout configuration is currently only supported on Sway and X11 window managers.",
        );
        return Ok(());
    }

    let layouts = match parse_xkb_layouts() {
        Ok(l) => l,
        Err(e) => {
            ctx.emit_info(
                "settings.keyboard.parse_error",
                &format!("Failed to parse keyboard layouts: {e}"),
            );
            return Ok(());
        }
    };

    let current_layout_key = if is_sway {
        super::super::store::StringSettingKey::new("language.keyboard.sway", "")
    } else {
        super::super::store::StringSettingKey::new("language.keyboard.x11", "")
    };

    let current_code = ctx.string(current_layout_key);

    // Find initial index if possible
    let initial_index = layouts.iter().position(|l| l.code == current_code);

    let result = FzfWrapper::builder()
        .header("Select Keyboard Layout")
        .prompt("Layout > ")
        .initial_index(initial_index.unwrap_or(0))
        .select(layouts)?;

    match result {
        FzfResult::Selected(layout) => {
            // Apply
            if is_sway {
                let cmd = format!("input type:keyboard xkb_layout {}", layout.code);
                if let Err(e) = sway::swaymsg(&cmd) {
                    ctx.emit_info(
                        "settings.keyboard.apply_error",
                        &format!("Failed to apply keyboard layout: {e}"),
                    );
                    return Ok(());
                }
            } else if is_x11
                && let Err(e) = std::process::Command::new("setxkbmap")
                    .arg(&layout.code)
                    .status()
            {
                ctx.emit_info(
                    "settings.keyboard.apply_error",
                    &format!("Failed to execute setxkbmap: {e}"),
                );
                return Ok(());
            }

            // Save to settings
            ctx.set_string(current_layout_key, &layout.code);

            ctx.notify(
                "Keyboard Layout",
                &format!("Set to: {} ({})", layout.name, layout.code),
            );
        }
        FzfResult::Error(e) => {
            bail!("fzf error: {e}");
        }
        _ => {}
    }

    Ok(())
}

pub fn restore_keyboard_layout(ctx: &mut SettingsContext) -> Result<()> {
    let compositor = CompositorType::detect();

    if matches!(compositor, CompositorType::Sway) {
        let key = super::super::store::StringSettingKey::new("language.keyboard.sway", "");
        let code = ctx.string(key);

        if !code.is_empty() {
            let cmd = format!("input type:keyboard xkb_layout {}", code);
            // We suppress errors here as this might run during startup/apply where we don't want to crash
            if let Err(e) = sway::swaymsg(&cmd) {
                emit(
                    Level::Warn,
                    "settings.keyboard.restore_failed",
                    &format!("Failed to restore Sway keyboard layout: {e}"),
                    None,
                );
            } else {
                emit(
                    Level::Debug,
                    "settings.keyboard.restored",
                    &format!("Restored Sway keyboard layout: {code}"),
                    None,
                );
            }
        }
    } else if compositor.is_x11() {
        let key = super::super::store::StringSettingKey::new("language.keyboard.x11", "");
        let code = ctx.string(key);

        if !code.is_empty() {
            if let Err(e) = std::process::Command::new("setxkbmap").arg(&code).status() {
                emit(
                    Level::Warn,
                    "settings.keyboard.restore_failed",
                    &format!("Failed to restore X11 keyboard layout: {e}"),
                    None,
                );
            } else {
                emit(
                    Level::Debug,
                    "settings.keyboard.restored",
                    &format!("Restored X11 keyboard layout: {code}"),
                    None,
                );
            }
        }
    }

    Ok(())
}

/// Apply swap escape/caps lock setting (X11 only for now)
pub fn apply_swap_escape(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();

    if !compositor.is_x11() {
        ctx.emit_info(
            "settings.keyboard.swap_escape.unsupported",
            &format!(
                "Swap Escape/Caps Lock configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    // Use setxkbmap -option to set or clear the caps:swapescape option
    // First, we need to get the current layout to preserve it
    let result = if enabled {
        // Apply the swap option
        std::process::Command::new("setxkbmap")
            .args(["-option", "caps:swapescape"])
            .status()
    } else {
        // Clear xkb options to reset to default
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

    Ok(())
}

pub fn restore_swap_escape(ctx: &mut SettingsContext) -> Result<()> {
    let compositor = CompositorType::detect();

    if !compositor.is_x11() {
        return Ok(());
    }

    let key = super::super::store::BoolSettingKey::new("desktop.swap_escape", false);
    let enabled = ctx.bool(key);

    if enabled {
        if let Err(e) = std::process::Command::new("setxkbmap")
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
    }

    Ok(())
}
