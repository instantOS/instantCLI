//! Shared keyboard utilities

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::common::compositor::{CompositorType, sway};
use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::settings::context::SettingsContext;
use crate::settings::store::StringSettingKey;
use crate::ui::catppuccin::{colors, format_icon};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;
use serde_json::Value;
use which::which;

pub struct KeyboardLayoutKeys {
    pub sway: StringSettingKey,
    pub x11: StringSettingKey,
    pub gnome: StringSettingKey,
    pub instantwm: StringSettingKey,
}

impl KeyboardLayoutKeys {
    pub fn new() -> Self {
        Self {
            sway: StringSettingKey::new("language.keyboard.sway", ""),
            x11: StringSettingKey::new("language.keyboard.x11", ""),
            gnome: StringSettingKey::new("language.keyboard.gnome", ""),
            instantwm: StringSettingKey::new("language.keyboard.instantwm", ""),
        }
    }
}

#[derive(Clone)]
pub struct LayoutChoice {
    pub code: String,
    pub name: String,
    pub checked: bool,
}

impl FzfSelectable for LayoutChoice {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", format_icon(NerdFont::Keyboard), self.name)
    }

    fn fzf_key(&self) -> String {
        self.code.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Keyboard, &self.name)
            .line(
                colors::TEAL,
                Some(NerdFont::Tag),
                &format!("Code: {}", self.code),
            )
            .build()
    }
}

pub fn parse_xkb_layouts() -> Result<Vec<LayoutChoice>> {
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
                layouts.push(LayoutChoice {
                    code,
                    name,
                    checked: false,
                });
            }
        }
    }

    Ok(layouts)
}

pub fn split_layout_codes(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for part in value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            result.push(trimmed.to_string());
        }
    }

    result
}

pub fn join_layout_codes(codes: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut cleaned = Vec::new();

    for code in codes {
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            cleaned.push(trimmed.to_string());
        }
    }

    cleaned.join(",")
}

pub fn current_x11_layouts() -> Vec<String> {
    let output = match Command::new("setxkbmap").arg("-query").output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("layout:") {
            return split_layout_codes(rest);
        }
    }

    Vec::new()
}

/// Get current GNOME keyboard layouts from gsettings
pub fn current_gnome_layouts() -> Option<Vec<String>> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.input-sources", "sources"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_gnome_sources(&stdout)
}

/// Parse GNOME sources string into layout codes
fn parse_gnome_sources(sources_str: &str) -> Option<Vec<String>> {
    let trimmed = sources_str.trim();
    if trimmed == "@as []" {
        return Some(Vec::new());
    }

    let content = trimmed.strip_prefix('[')?.strip_suffix(']')?.trim();
    if content.is_empty() {
        return Some(Vec::new());
    }

    let mut layouts = Vec::new();
    for tuple in content.split("), (") {
        let clean = tuple
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim()
            .trim_matches('\'');

        let parts: Vec<&str> = clean.split("', '").collect();
        if parts.len() == 2 {
            let layout_code = parts[1].trim().trim_matches('\'');
            if !layout_code.is_empty() {
                layouts.push(layout_code.to_string());
            }
        }
    }

    if layouts.is_empty() {
        None
    } else {
        Some(layouts)
    }
}

fn current_x11_options() -> Option<String> {
    let output = Command::new("setxkbmap").arg("-query").output().ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("options:") {
            let options = rest.trim();
            if options.is_empty() {
                return None;
            }
            return Some(options.to_string());
        }
    }

    None
}

pub fn current_sway_layout_names() -> Option<Vec<String>> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_inputs", "-r"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let data: Value = serde_json::from_slice(&output.stdout).ok()?;
    let inputs = data.as_array()?;

    let mut names = Vec::new();
    let mut seen = HashSet::new();

    for input in inputs {
        if input.get("type").and_then(|v| v.as_str()) != Some("keyboard") {
            continue;
        }

        if let Some(layouts) = input.get("xkb_layout_names").and_then(|v| v.as_array()) {
            for layout in layouts {
                if let Some(name) = layout.as_str()
                    && seen.insert(name.to_string())
                {
                    names.push(name.to_string());
                }
            }
        }
    }

    if names.is_empty() { None } else { Some(names) }
}

pub fn current_instantwm_layouts() -> Option<Vec<String>> {
    let output = Command::new("instantwmctl")
        .args(["keyboard", "list"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut layouts = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let layout = line.trim_start_matches("* ").trim();
        if let Some(name) = layout.split_whitespace().next() {
            layouts.push(name.to_string());
        }
    }

    if layouts.is_empty() {
        None
    } else {
        Some(layouts)
    }
}

pub fn map_layout_names_to_codes(names: &[String], layouts: &[LayoutChoice]) -> Vec<String> {
    let mut map = HashMap::new();
    for layout in layouts {
        map.insert(layout.name.clone(), layout.code.clone());
    }

    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        if let Some(code) = map.get(name)
            && seen.insert(code.clone())
        {
            result.push(code.clone());
        }
    }

    result
}

pub fn list_keymaps() -> Result<Vec<String>> {
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

pub fn current_vconsole_keymap() -> Option<String> {
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

pub fn current_x11_layout() -> Option<String> {
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

pub fn ensure_localectl(ctx: &mut SettingsContext, code: &str, message: &str) -> bool {
    if which("localectl").is_err() {
        ctx.emit_unsupported(code, message);
        return false;
    }
    true
}

/// Apply keyboard layout(s) via swaymsg, setxkbmap, instantwmctl, or gsettings depending on compositor
pub fn apply_keyboard_layouts(codes: &[String], compositor: &CompositorType) -> Result<()> {
    let joined = join_layout_codes(codes);
    if joined.is_empty() {
        bail!("No keyboard layouts selected");
    }

    match compositor {
        CompositorType::Sway => {
            let cmd = format!("input type:keyboard xkb_layout \"{joined}\"");
            sway::swaymsg(&cmd)?;
        }
        CompositorType::Gnome => {
            apply_gnome_keyboard_layouts(codes)?;
        }
        CompositorType::InstantWM => {
            let mut cmd = Command::new("instantwmctl");
            cmd.args(["keyboard", "set"]);
            for code in codes {
                cmd.arg(code);
            }
            cmd.status().with_context(|| {
                format!("Failed to execute instantwmctl keyboard set for layout '{joined}'")
            })?;
        }
        _ if compositor.is_x11() => {
            let mut command = Command::new("setxkbmap");
            command.arg("-layout").arg(&joined);
            if let Some(options) = current_x11_options() {
                command.arg("-option").arg(options);
            }
            command
                .status()
                .with_context(|| format!("Failed to execute setxkbmap for layout '{joined}'"))?;
        }
        _ => bail!("Unsupported compositor for keyboard layout configuration"),
    }
    Ok(())
}

/// Apply keyboard layouts to GNOME via gsettings
pub fn apply_gnome_keyboard_layouts(codes: &[String]) -> Result<()> {
    let sources: Vec<String> = codes
        .iter()
        .map(|code| format!("('xkb', '{}')", code))
        .collect();
    let sources_str = format!("[{}]", sources.join(", "));

    std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.input-sources",
            "sources",
            &sources_str,
        ])
        .status()
        .with_context(|| format!("Failed to set GNOME keyboard layouts to: {sources_str}"))?;

    Ok(())
}
