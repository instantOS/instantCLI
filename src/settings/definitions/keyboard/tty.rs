//! TTY keymap setting

use anyhow::{bail, Result};

use crate::arch::annotations::{KeymapAnnotationProvider, annotate_list};
use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
use crate::ui::prelude::NerdFont;

use super::common::{current_vconsole_keymap, ensure_localectl, list_keymaps};

pub struct TtyKeymap;

impl TtyKeymap {
    const KEY: StringSettingKey = StringSettingKey::new("language.keyboard.tty", "");
}

impl Setting for TtyKeymap {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.keyboard_tty")
            .title("TTY Keymap")
            .icon(NerdFont::Keyboard)
            .summary("Set the console (TTY) keymap via localectl set-keymap.\n\nAffects virtual consoles and login TTYs.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        if !ensure_localectl(
            ctx,
            "settings.keyboard.tty.no_systemd",
            "TTY keymap configuration requires systemd (localectl not found).",
        ) {
            return Ok(());
        }

        let keymaps = match list_keymaps() {
            Ok(list) => list,
            Err(err) => {
                ctx.emit_info(
                    "settings.keyboard.tty.list_failed",
                    &format!("Failed to list keymaps: {err}"),
                );
                return Ok(());
            }
        };

        if keymaps.is_empty() {
            ctx.emit_info(
                "settings.keyboard.tty.none",
                "No console keymaps reported by localectl.",
            );
            return Ok(());
        }

        let provider = KeymapAnnotationProvider;
        let choices = annotate_list(Some(&provider), keymaps);

        let current = if ctx.contains(Self::KEY.key) {
            ctx.string(Self::KEY)
        } else {
            current_vconsole_keymap().unwrap_or_default()
        };

        let initial_index = choices
            .iter()
            .position(|choice| choice.value == current)
            .unwrap_or(0);

        let result = FzfWrapper::builder()
            .header("Select TTY Keymap")
            .prompt("Keymap")
            .initial_index(initial_index)
            .select(choices)?;

        match result {
            FzfResult::Selected(choice) => {
                if let Err(err) =
                    ctx.run_command_as_root("localectl", ["set-keymap", choice.value.as_str()])
                {
                    ctx.emit_info(
                        "settings.keyboard.tty.apply_failed",
                        &format!("Failed to apply TTY keymap: {err}"),
                    );
                    return Ok(());
                }

                ctx.set_string(Self::KEY, &choice.value);
                ctx.notify("TTY Keymap", &format!("Set to: {}", choice.value));
            }
            FzfResult::Error(err) => bail!("fzf error: {err}"),
            _ => {}
        }

        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::TtyKeymap))
    }
}
