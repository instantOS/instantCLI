//! Keyboard layout setting

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::common::compositor::{CompositorType, sway};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
use crate::ui::prelude::*;

pub struct KeyboardLayout;

impl KeyboardLayout {
    const KEY_SWAY: StringSettingKey = StringSettingKey::new("language.keyboard.sway", "");
    const KEY_X11: StringSettingKey = StringSettingKey::new("language.keyboard.x11", "");
}

/// Apply keyboard layout via swaymsg or setxkbmap depending on compositor
fn apply_keyboard_layout(code: &str, compositor: &CompositorType) -> Result<()> {
    if matches!(compositor, CompositorType::Sway) {
        let cmd = format!("input type:keyboard xkb_layout {code}");
        sway::swaymsg(&cmd)?;
    } else if compositor.is_x11() {
        std::process::Command::new("setxkbmap")
            .arg(code)
            .status()
            .with_context(|| format!("Failed to execute setxkbmap for layout '{code}'"))?;
    }
    Ok(())
}

impl Setting for KeyboardLayout {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.keyboard_layout")
            .title("Keyboard Layout")
            .icon(NerdFont::Keyboard)
            .summary("Select and set the keyboard layout (e.g., us, de, fr).\n\nSupports Sway and X11 window managers.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
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
            Self::KEY_SWAY
        } else {
            Self::KEY_X11
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
                if let Err(e) = apply_keyboard_layout(&layout.code, &compositor) {
                    ctx.emit_info(
                        "settings.keyboard.apply_error",
                        &format!("Failed to apply keyboard layout: {e}"),
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

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let compositor = CompositorType::detect();
        let is_sway = matches!(compositor, CompositorType::Sway);
        let is_x11 = compositor.is_x11();

        if !is_sway && !is_x11 {
            return None;
        }

        let key = if is_sway {
            Self::KEY_SWAY
        } else {
            Self::KEY_X11
        };
        let code = ctx.string(key);
        if code.is_empty() {
            return None;
        }

        if let Err(e) = apply_keyboard_layout(&code, &compositor) {
            emit(
                Level::Warn,
                "settings.keyboard.restore_failed",
                &format!("Failed to restore keyboard layout: {e}"),
                None,
            );
        } else {
            emit(
                Level::Debug,
                "settings.keyboard.restored",
                &format!("Restored keyboard layout: {code}"),
                None,
            );
        }

        Some(Ok(()))
    }
}
