use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde::Deserialize;

use crate::preview::helpers::{push_raw_lines, truncate_label};
use crate::settings::definitions::keyboard::{
    current_gnome_layouts, current_vconsole_keymap, current_x11_layout, current_x11_layouts,
};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_keyboard_layout_preview() -> Result<String> {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Keyboard, "Keyboard Layout")
        .subtext("Active layout for the current desktop session.")
        .blank()
        .text("Current State");

    let lines = if is_sway_session() {
        sway_keyboard_lines()
    } else if is_gnome_session() {
        gnome_keyboard_lines()
    } else if is_x11_session() {
        x11_keyboard_lines()
    } else {
        vec![
            "Session: Unsupported".to_string(),
            "Layout: Not detected".to_string(),
        ]
    };

    builder = push_raw_lines(builder, &lines)
        .blank()
        .text("Info")
        .text("Sway can have multiple layouts per keyboard.")
        .text("Applies via swaymsg (Sway), gsettings (GNOME), or setxkbmap (X11).")
        .text("Saved value is reapplied on login.");

    Ok(builder.build_string())
}

pub(crate) fn render_tty_keymap_preview() -> Result<String> {
    let keymap = current_vconsole_keymap().unwrap_or_else(|| "Not set".to_string());

    let builder = PreviewBuilder::new()
        .header(NerdFont::Keyboard, "TTY Keymap")
        .subtext("Console keymap for virtual terminals.")
        .blank()
        .text("Current State")
        .raw(&format!("Keymap: {keymap}"))
        .raw("File: /etc/vconsole.conf")
        .blank()
        .text("Info")
        .text("Applies to virtual consoles and login TTYs.")
        .text("Configured via localectl set-keymap.")
        .build_string();

    Ok(builder)
}

pub(crate) fn render_login_screen_layout_preview() -> Result<String> {
    let mut layout = None;
    let mut source = None;
    let config_path = Path::new("/etc/X11/xorg.conf.d/00-keyboard.conf");

    if let Some(value) = read_xkb_layout_from_xorg(config_path) {
        layout = Some(value);
        source = Some("/etc/X11/xorg.conf.d/00-keyboard.conf".to_string());
    }

    if layout.is_none() {
        layout = current_x11_layout();
        if layout.is_some() {
            source = Some("localectl status".to_string());
        }
    }

    let layout = layout.unwrap_or_else(|| "Not set".to_string());

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Keyboard, "Login Screen Layout")
        .subtext("X11 layout used by GDM/LightDM login screens.")
        .blank()
        .text("Current State")
        .raw(&format!("Layout: {layout}"));

    if let Some(source) = source {
        builder = builder.raw(&format!("Source: {source}"));
    }

    builder = builder
        .blank()
        .text("Info")
        .text("Configured via localectl set-x11-keymap.");

    Ok(builder.build_string())
}

fn is_sway_session() -> bool {
    std::env::var_os("SWAYSOCK").is_some() && which::which("swaymsg").is_ok()
}

fn is_gnome_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|s| s.to_lowercase().contains("gnome"))
        .unwrap_or(false)
        || std::env::var("DESKTOP_SESSION")
            .map(|s| s.to_lowercase().contains("gnome"))
            .unwrap_or(false)
}

fn is_x11_session() -> bool {
    std::env::var_os("DISPLAY").is_some() && which::which("setxkbmap").is_ok()
}

fn gnome_keyboard_lines() -> Vec<String> {
    let layouts = current_gnome_layouts().unwrap_or_default();
    let layout = layouts
        .first()
        .cloned()
        .unwrap_or_else(|| "Not detected".to_string());

    let mut lines = vec!["Session: GNOME".to_string(), format!("Layout: {layout}")];
    if layouts.len() > 1 {
        lines.push(format!("Layouts: {}", layouts.join(", ")));
    }
    lines
}

fn sway_keyboard_lines() -> Vec<String> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_inputs", "-r"])
        .output();

    let Ok(output) = output else {
        return vec![
            "Session: Sway".to_string(),
            "Active: Not detected".to_string(),
            "Layouts: Not detected".to_string(),
            "Keyboards: 0".to_string(),
        ];
    };

    if !output.status.success() {
        return vec![
            "Session: Sway".to_string(),
            "Active: Not detected".to_string(),
            "Layouts: Not detected".to_string(),
            "Keyboards: 0".to_string(),
        ];
    }

    let inputs: Result<Vec<SwayInput>, _> = serde_json::from_slice(&output.stdout);
    let Ok(inputs) = inputs else {
        return vec![
            "Session: Sway".to_string(),
            "Active: Not detected".to_string(),
            "Layouts: Not detected".to_string(),
            "Keyboards: 0".to_string(),
        ];
    };

    let keyboards: Vec<SwayInput> = inputs
        .into_iter()
        .filter(|input| input.input_type.as_deref() == Some("keyboard"))
        .collect();

    if keyboards.is_empty() {
        return vec![
            "Session: Sway".to_string(),
            "Active: Not detected".to_string(),
            "Layouts: Not detected".to_string(),
            "Keyboards: 0".to_string(),
        ];
    }

    let mut unique_layouts = Vec::new();
    let mut unique_active = Vec::new();
    let mut seen_layouts = HashSet::new();
    let mut seen_active = HashSet::new();
    let mut device_lines = Vec::new();

    for dev in &keyboards {
        let layouts = dev.xkb_layout_names.clone().unwrap_or_default();
        let active_index = dev
            .xkb_active_layout_index
            .and_then(|idx| if idx >= 0 { Some(idx as usize) } else { None });
        let active_name = dev.xkb_active_layout_name.clone();

        if let Some(ref name) = active_name
            && seen_active.insert(name.clone())
        {
            unique_active.push(name.clone());
        }

        for layout in &layouts {
            if seen_layouts.insert(layout.clone()) {
                unique_layouts.push(layout.clone());
            }
        }

        let active_label = if let Some(name) = active_name {
            name
        } else if let Some(idx) = active_index {
            layouts
                .get(idx)
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string())
        } else if let Some(first) = layouts.first() {
            first.clone()
        } else {
            "Unknown".to_string()
        };

        let suffix = if let Some(idx) = active_index {
            if !layouts.is_empty() {
                format!(" ({}/{})", idx + 1, layouts.len())
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let name = truncate_label(dev.name.as_deref().unwrap_or("Keyboard"), 40);
        device_lines.push(format!("{name}: {active_label}{suffix}"));
    }

    let active_label = if !unique_active.is_empty() {
        unique_active.join(", ")
    } else if !unique_layouts.is_empty() {
        unique_layouts.join(", ")
    } else {
        "Unknown".to_string()
    };

    let layouts_label = if !unique_layouts.is_empty() {
        unique_layouts.join(", ")
    } else {
        "Unknown".to_string()
    };

    let mut lines = vec![
        "Session: Sway".to_string(),
        format!("Active: {active_label}"),
        format!("Layouts: {layouts_label}"),
        format!("Keyboards: {}", keyboards.len()),
    ];

    for line in device_lines.iter().take(3) {
        lines.push(format!("  - {line}"));
    }
    if device_lines.len() > 3 {
        lines.push(format!("  - ... and {} more", device_lines.len() - 3));
    }

    lines
}

fn x11_keyboard_lines() -> Vec<String> {
    let layouts = current_x11_layouts();
    let layout = layouts
        .first()
        .cloned()
        .unwrap_or_else(|| "Not detected".to_string());

    let mut lines = vec!["Session: X11".to_string(), format!("Layout: {layout}")];
    if !layouts.is_empty() {
        lines.push(format!("Layouts: {}", layouts.join(", ")));
    }
    lines
}

fn read_xkb_layout_from_xorg(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("XkbLayout") {
            continue;
        }
        if let Some(value) = extract_quoted_value(trimmed)
            && !value.is_empty()
        {
            return Some(value);
        }
    }

    None
}

fn extract_quoted_value(input: &str) -> Option<String> {
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut value = String::new();
            for next in chars.by_ref() {
                if next == '"' {
                    return Some(value);
                }
                value.push(next);
            }
            return None;
        }
    }
    None
}

#[derive(Debug, Clone, Deserialize)]
struct SwayInput {
    #[serde(rename = "type")]
    input_type: Option<String>,
    name: Option<String>,
    xkb_layout_names: Option<Vec<String>>,
    xkb_active_layout_index: Option<i64>,
    xkb_active_layout_name: Option<String>,
}
