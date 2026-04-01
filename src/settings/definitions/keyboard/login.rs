//! Login screen keyboard layout setting

use anyhow::{Result, bail};

use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
use crate::ui::prelude::NerdFont;

use super::common::{current_x11_layout, ensure_localectl, parse_xkb_layouts};

pub struct LoginScreenLayout;

impl LoginScreenLayout {
    const KEY: StringSettingKey = StringSettingKey::new("language.keyboard.login", "");
}

impl Setting for LoginScreenLayout {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.keyboard_login")
            .title("Login Screen Layout")
            .icon(NerdFont::Keyboard)
            .summary("Set the keyboard layout for GDM/LightDM login screens via localectl set-x11-keymap.\n\nAffects the display manager and default X11 layout.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        if !ensure_localectl(
            ctx,
            "settings.keyboard.login.no_systemd",
            "Login screen layout configuration requires systemd (localectl not found).",
        ) {
            return Ok(());
        }

        let layouts = match parse_xkb_layouts() {
            Ok(list) => list,
            Err(err) => {
                ctx.emit_info(
                    "settings.keyboard.login.parse_failed",
                    &format!("Failed to parse XKB layouts: {err}"),
                );
                return Ok(());
            }
        };

        if layouts.is_empty() {
            ctx.emit_info(
                "settings.keyboard.login.none",
                "No XKB layouts found. Ensure xkeyboard-config is installed.",
            );
            return Ok(());
        }

        let current = if ctx.contains(Self::KEY.key) {
            ctx.string(Self::KEY)
        } else {
            current_x11_layout().unwrap_or_default()
        };

        let initial_index = layouts
            .iter()
            .position(|layout| layout.code == current)
            .unwrap_or(0);

        let result = FzfWrapper::builder()
            .header("Select Login Screen Layout")
            .prompt("Layout")
            .initial_index(initial_index)
            .select(layouts)?;

        match result {
            FzfResult::Selected(layout) => {
                if let Err(err) =
                    ctx.run_command_as_root("localectl", ["set-x11-keymap", layout.code.as_str()])
                {
                    ctx.emit_info(
                        "settings.keyboard.login.apply_failed",
                        &format!("Failed to apply login screen layout: {err}"),
                    );
                    return Ok(());
                }

                ctx.set_string(Self::KEY, &layout.code);
                ctx.notify(
                    "Login Screen Layout",
                    &format!("Set to: {} ({})", layout.name, layout.code),
                );
            }
            FzfResult::Error(err) => bail!("fzf error: {err}"),
            _ => {}
        }

        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::LoginScreenLayout))
    }
}
