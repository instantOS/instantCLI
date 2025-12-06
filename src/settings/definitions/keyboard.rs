//! Keyboard layout setting

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::common::compositor::{CompositorType, sway};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
use crate::ui::prelude::*;

pub struct KeyboardLayout;

impl KeyboardLayout {
    const KEY_SWAY: StringSettingKey = StringSettingKey::new("language.keyboard.sway", "");
    const KEY_X11: StringSettingKey = StringSettingKey::new("language.keyboard.x11", "");
}

impl Setting for KeyboardLayout {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "language.keyboard_layout",
            title: "Keyboard Layout",
            category: Category::Language,
            icon: NerdFont::Keyboard,
            breadcrumbs: &["Language", "Keyboard Layout"],
            summary: "Select and set the keyboard layout (e.g., us, de, fr).\n\nSupports Sway and X11 window managers.",
            requires_reapply: true,
            requirements: &[],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        configure_keyboard_layout_impl(ctx)
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        Some(restore_keyboard_layout_impl(ctx))
    }
}

inventory::submit! { &KeyboardLayout as &'static dyn Setting }

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

fn configure_keyboard_layout_impl(ctx: &mut SettingsContext) -> Result<()> {
    let compositor = CompositorType::detect();
    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_x11 = compositor.is_x11();

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
        KeyboardLayout::KEY_SWAY
    } else {
        KeyboardLayout::KEY_X11
    };

    let current_code = ctx.string(current_layout_key);
    let initial_index = layouts.iter().position(|l| l.code == current_code);

    let result = FzfWrapper::builder()
        .header("Select Keyboard Layout")
        .prompt("Layout > ")
        .initial_index(initial_index.unwrap_or(0))
        .select(layouts)?;

    match result {
        FzfResult::Selected(layout) => {
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

fn restore_keyboard_layout_impl(ctx: &mut SettingsContext) -> Result<()> {
    let compositor = CompositorType::detect();

    if matches!(compositor, CompositorType::Sway) {
        let code = ctx.string(KeyboardLayout::KEY_SWAY);
        if !code.is_empty() {
            let cmd = format!("input type:keyboard xkb_layout {}", code);
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
        let code = ctx.string(KeyboardLayout::KEY_X11);
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
