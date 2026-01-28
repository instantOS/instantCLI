//! Keyboard layout setting

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::Command;

use crate::arch::annotations::{annotate_list, KeymapAnnotationProvider};
use crate::common::compositor::{sway, CompositorType};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
use crate::ui::prelude::*;
use which::which;

pub struct KeyboardLayout;
pub struct TtyKeymap;
pub struct LoginScreenLayout;

impl KeyboardLayout {
    const KEY_SWAY: StringSettingKey = StringSettingKey::new("language.keyboard.sway", "");
    const KEY_X11: StringSettingKey = StringSettingKey::new("language.keyboard.x11", "");
}

impl TtyKeymap {
    const KEY: StringSettingKey = StringSettingKey::new("language.keyboard.tty", "");
}

impl LoginScreenLayout {
    const KEY: StringSettingKey = StringSettingKey::new("language.keyboard.login", "");
}

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

fn list_keymaps() -> Result<Vec<String>> {
    let output = Command::new("localectl")
        .arg("list-keymaps")
        .output()
        .context("running localectl list-keymaps")?;

    if !output.status.success() {
        bail!(
            "localectl list-keymaps exited with status {:?}",
            output.status.code()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

fn current_vconsole_keymap() -> Option<String> {
    std::fs::read_to_string("/etc/vconsole.conf")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.trim_start().starts_with("KEYMAP="))
                .map(|line| {
                    line.trim_start()
                        .trim_start_matches("KEYMAP=")
                        .trim()
                        .to_string()
                })
        })
}

fn current_x11_layout() -> Option<String> {
    let output = Command::new("localectl").arg("status").output().ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("X11 Layout:") {
            let layout = rest.trim();
            if layout.is_empty() {
                return None;
            }
            let first = layout.split(',').next().unwrap_or(layout).trim();
            if first.is_empty() {
                return None;
            }
            return Some(first.to_string());
        }
    }

    None
}

fn ensure_localectl(ctx: &mut SettingsContext, code: &str, message: &str) -> bool {
    if which("localectl").is_err() {
        ctx.emit_unsupported(code, message);
        return false;
    }
    true
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
            .summary("Select the keyboard layout for the current desktop session (e.g., us, de, fr).\n\nSupports Sway and X11 window managers. Use the TTY and login screen settings for system-wide layouts.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let compositor = CompositorType::detect();
        let is_sway = matches!(compositor, CompositorType::Sway);
        let is_x11 = compositor.is_x11();

        if !is_sway && !is_x11 {
            ctx.emit_unsupported(
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
}
