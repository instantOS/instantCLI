use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::Deserialize;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::shell::shell_quote;
use crate::settings::defaultapps::{
    build_mime_to_apps_map, get_application_info, get_apps_for_mime, query_default_app,
    ARCHIVE_MIME_TYPES, AUDIO_MIME_TYPES, IMAGE_MIME_TYPES, VIDEO_MIME_TYPES,
};
use crate::settings::definitions::appearance::common::{
    get_current_gtk_theme, get_current_icon_theme, icon_theme_exists, is_dark_mode, theme_exists,
};
use crate::settings::definitions::keyboard::{
    current_vconsole_keymap, current_x11_layout, current_x11_layouts,
};
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PreviewId {
    #[value(name = "keyboard-layout")]
    KeyboardLayout,
    #[value(name = "tty-keymap")]
    TtyKeymap,
    #[value(name = "login-screen-layout")]
    LoginScreenLayout,
    #[value(name = "timezone")]
    Timezone,
    #[value(name = "mime-type")]
    MimeType,
    #[value(name = "bluetooth")]
    Bluetooth,
    #[value(name = "dark-mode")]
    DarkMode,
    #[value(name = "gtk-theme")]
    GtkTheme,
    #[value(name = "icon-theme")]
    IconTheme,
    #[value(name = "default-image-viewer")]
    DefaultImageViewer,
    #[value(name = "default-video-player")]
    DefaultVideoPlayer,
    #[value(name = "default-audio-player")]
    DefaultAudioPlayer,
    #[value(name = "default-archive-manager")]
    DefaultArchiveManager,
    #[value(name = "disk")]
    Disk,
    #[value(name = "partition")]
    Partition,
}

impl PreviewId {
    pub fn as_str(&self) -> &'static str {
        match self {
            PreviewId::KeyboardLayout => "keyboard-layout",
            PreviewId::TtyKeymap => "tty-keymap",
            PreviewId::LoginScreenLayout => "login-screen-layout",
            PreviewId::Timezone => "timezone",
            PreviewId::MimeType => "mime-type",
            PreviewId::Bluetooth => "bluetooth",
            PreviewId::DarkMode => "dark-mode",
            PreviewId::GtkTheme => "gtk-theme",
            PreviewId::IconTheme => "icon-theme",
            PreviewId::DefaultImageViewer => "default-image-viewer",
            PreviewId::DefaultVideoPlayer => "default-video-player",
            PreviewId::DefaultAudioPlayer => "default-audio-player",
            PreviewId::DefaultArchiveManager => "default-archive-manager",
            PreviewId::Disk => "disk",
            PreviewId::Partition => "partition",
        }
    }
}

impl std::fmt::Display for PreviewId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn preview_command(id: PreviewId) -> String {
    let exe = current_exe_command();
    format!("{exe} preview --id {} --key \"$1\"", id.as_str())
}

pub fn handle_preview_command(id: PreviewId, key: Option<String>) -> Result<()> {
    let ctx = PreviewContext {
        key,
        columns: env_usize("FZF_PREVIEW_COLUMNS"),
        lines: env_usize("FZF_PREVIEW_LINES"),
    };

    let output = match render_preview(id, &ctx) {
        Ok(text) => text,
        Err(err) => render_error_preview(id, err),
    };

    print!("{output}");
    Ok(())
}

struct PreviewContext {
    key: Option<String>,
    #[allow(dead_code)]
    columns: Option<usize>,
    #[allow(dead_code)]
    lines: Option<usize>,
}

fn render_preview(id: PreviewId, ctx: &PreviewContext) -> Result<String> {
    match id {
        PreviewId::KeyboardLayout => render_keyboard_layout_preview(),
        PreviewId::TtyKeymap => render_tty_keymap_preview(),
        PreviewId::LoginScreenLayout => render_login_screen_layout_preview(),
        PreviewId::Timezone => render_timezone_preview(ctx),
        PreviewId::MimeType => render_mime_type_preview(ctx),
        PreviewId::Bluetooth => render_bluetooth_preview(),
        PreviewId::DarkMode => render_dark_mode_preview(),
        PreviewId::GtkTheme => render_gtk_theme_preview(),
        PreviewId::IconTheme => render_icon_theme_preview(),
        PreviewId::DefaultImageViewer => render_default_app_preview(
            "Image Viewer",
            NerdFont::Image,
            "Set your default image viewer for photos and pictures.",
            IMAGE_MIME_TYPES,
        ),
        PreviewId::DefaultVideoPlayer => render_default_app_preview(
            "Video Player",
            NerdFont::Video,
            "Set your default video player for movies and videos.",
            VIDEO_MIME_TYPES,
        ),
        PreviewId::DefaultAudioPlayer => render_default_app_preview(
            "Audio Player",
            NerdFont::Music,
            "Set your default audio player for music and podcasts.",
            AUDIO_MIME_TYPES,
        ),
        PreviewId::DefaultArchiveManager => render_default_app_preview(
            "Archive Manager",
            NerdFont::Archive,
            "Set your default archive manager for ZIP, TAR, and other compressed files.",
            ARCHIVE_MIME_TYPES,
        ),
        PreviewId::Disk => render_disk_preview(ctx),
        PreviewId::Partition => render_partition_preview(ctx),
    }
}

fn render_error_preview(id: PreviewId, err: anyhow::Error) -> String {
    PreviewBuilder::new()
        .header(NerdFont::Warning, "Preview Unavailable")
        .subtext(&format!("Failed to render '{id}'"))
        .blank()
        .text(&err.to_string())
        .build_string()
}

fn render_timezone_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(tz) = ctx.key.as_deref().filter(|k| !k.is_empty()) else {
        return Ok(String::new());
    };

    let current_local =
        date_in_tz(tz, "+%Y-%m-%d %H:%M:%S %Z").unwrap_or_else(|| "Unavailable".to_string());
    let day_line = date_in_tz(tz, "+%A, %d %B %Y").unwrap_or_else(|| "Unavailable".to_string());
    let twelve_hour = date_in_tz(tz, "+%I:%M %p").unwrap_or_else(|| "Unavailable".to_string());
    let twenty_four = date_in_tz(tz, "+%H:%M").unwrap_or_else(|| "Unavailable".to_string());
    let offset_raw = date_in_tz(tz, "+%z").unwrap_or_else(|| "Unknown".to_string());
    let local_system =
        date_local("+%Y-%m-%d %H:%M:%S %Z").unwrap_or_else(|| "Unavailable".to_string());

    let offset = format_utc_offset(&offset_raw);

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Clock, "Timezone")
        .subtext("Preview of the selected timezone.")
        .blank()
        .field("Timezone", tz)
        .blank()
        .line(colors::TEAL, None, "Current Time")
        .raw(&format!("  {current_local}"))
        .raw(&format!("  {day_line}"))
        .blank()
        .line(colors::TEAL, None, "UTC Offset")
        .raw(&format!("  {offset}"))
        .blank()
        .line(colors::TEAL, None, "Formats")
        .raw(&format!("  12-hour: {twelve_hour}"))
        .raw(&format!("  24-hour: {twenty_four}"))
        .blank()
        .line(colors::TEAL, None, "Local System Time")
        .raw(&format!("  {local_system}"));

    Ok(builder.build_string())
}

fn render_keyboard_layout_preview() -> Result<String> {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Keyboard, "Keyboard Layout")
        .subtext("Active layout for the current desktop session.")
        .blank()
        .text("Current State");

    let lines = if is_sway_session() {
        sway_keyboard_lines()
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
        .text("Applies via swaymsg (Sway) or setxkbmap (X11).")
        .text("Saved value is reapplied on login.");

    Ok(builder.build_string())
}

fn render_tty_keymap_preview() -> Result<String> {
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

fn render_login_screen_layout_preview() -> Result<String> {
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

fn render_mime_type_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(mime_type) = ctx.key.as_deref().filter(|k| !k.is_empty()) else {
        return Ok(String::new());
    };

    let category = mime_category(mime_type);
    let extensions = mime_extensions(mime_type);
    let default = query_default_app(mime_type)
        .ok()
        .flatten()
        .map(|desktop_id| display_app_name(&desktop_id))
        .unwrap_or_else(|| "(not set)".to_string());

    let app_map = build_mime_to_apps_map().unwrap_or_default();
    let mut apps = get_apps_for_mime(mime_type, &app_map);
    let current_default = query_default_app(mime_type).ok().flatten();

    apps.sort();
    let app_lines: Vec<String> = apps
        .into_iter()
        .take(8)
        .map(|desktop_id| {
            let label = display_app_name(&desktop_id);
            if current_default.as_deref() == Some(desktop_id.as_str()) {
                format!("{label} (current)")
            } else {
                label
            }
        })
        .collect();

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::File, "MIME Type")
        .subtext("Select a default application for this MIME type.")
        .blank()
        .line(colors::TEAL, None, "▸ Details")
        .field_indented("Type", mime_type)
        .raw(&format!("  Category: {category}"))
        .blank()
        .line(colors::TEAL, None, "▸ Common Extensions");

    if extensions.is_empty() {
        builder = builder.bullet("(none registered)");
    } else {
        builder = push_bullets(builder, &extensions);
    }

    builder = builder
        .blank()
        .line(colors::TEAL, None, "▸ Current Default")
        .field_indented(mime_type, &default)
        .blank()
        .line(colors::TEAL, None, "▸ Available Applications");

    if app_lines.is_empty() {
        builder = builder.bullet("(none registered)");
    } else {
        builder = push_bullets(builder, &app_lines);
    }

    Ok(builder.build_string())
}

fn render_bluetooth_preview() -> Result<String> {
    let green = hex_to_ansi_fg(colors::GREEN);
    let red = hex_to_ansi_fg(colors::RED);
    let teal = hex_to_ansi_fg(colors::TEAL);
    let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
    let mauve = hex_to_ansi_fg(colors::MAUVE);
    let reset = "\x1b[0m";

    let systemd = crate::common::systemd::SystemdManager::system();
    let active = systemd.is_active("bluetooth.service");
    let enabled = systemd.is_enabled("bluetooth.service");

    let mut builder = PreviewBuilder::new()
        .line(colors::MAUVE, Some(NerdFont::Bluetooth), "Bluetooth")
        .separator()
        .blank()
        .text("Turn Bluetooth on or off.")
        .blank()
        .line(colors::TEAL, None, "▸ Features")
        .bullets([
            "Connect wireless headphones & speakers",
            "Pair keyboards and mice",
            "Transfer files between devices",
        ])
        .blank()
        .line(colors::TEAL, None, "▸ Current Status");

    if active {
        builder = builder.raw(&format!("  {green}● Bluetooth service is running{reset}"));
    } else {
        builder = builder.raw(&format!("  {red}○ Bluetooth service is stopped{reset}"));
    }

    if enabled {
        builder = builder.raw(&format!("  {teal}  Enabled at boot{reset}"));
    } else {
        builder = builder.raw(&format!("  {subtext}  Disabled at boot{reset}"));
    }

    builder = builder
        .blank()
        .line(colors::TEAL, None, "▸ Connected Devices");

    if which::which("bluetoothctl").is_err() {
        builder = builder.raw(&format!("  {subtext}bluetoothctl not installed{reset}"));
        return Ok(builder.build_string());
    }

    let devices = bluetooth_connected_devices();
    if devices.is_empty() {
        builder = builder.raw(&format!("  {subtext}No devices connected{reset}"));
    } else {
        for device in devices {
            builder = builder.raw(&format!("  {mauve}•{reset} {device}"));
        }
    }

    Ok(builder.build_string())
}

fn render_dark_mode_preview() -> Result<String> {
    let is_dark = is_dark_mode().unwrap_or(false);
    let mode = if is_dark { "Dark" } else { "Light" };

    let gtk_theme = get_current_gtk_theme().unwrap_or_else(|_| "unknown".to_string());
    let icon_theme = get_current_icon_theme().unwrap_or_else(|_| "unknown".to_string());

    let target_mode = if is_dark { "Light" } else { "Dark" };
    let (new_gtk, gtk_changed) = find_theme_variant(&gtk_theme, !is_dark, theme_exists);
    let (new_icon, icon_changed) = find_theme_variant(&icon_theme, !is_dark, icon_theme_exists);

    let mut builder = PreviewBuilder::new()
        .line(colors::MAUVE, Some(NerdFont::Moon), "Dark Mode")
        .separator()
        .blank()
        .text("Switch applications between light and dark variants.")
        .text("Updates GTK and icon themes when paired variants exist.")
        .text("Sets the GTK 4 color-scheme preference for compatible apps.")
        .blank()
        .subtext("Current state")
        .raw(&format!("  Mode: {mode}"))
        .raw(&format!("  GTK theme: {gtk_theme}"))
        .raw(&format!("  Icon theme: {icon_theme}"))
        .blank()
        .separator()
        .blank()
        .raw(&format!("  Will switch to: {target_mode} mode"));

    if gtk_changed {
        builder = builder.raw(&format!("  GTK: {gtk_theme} → {new_gtk}"));
    }
    if icon_changed {
        builder = builder.raw(&format!("  Icons: {icon_theme} → {new_icon}"));
    }

    Ok(builder.build_string())
}

fn render_gtk_theme_preview() -> Result<String> {
    let theme = get_current_gtk_theme().unwrap_or_else(|_| "unknown".to_string());
    Ok(PreviewBuilder::new()
        .header(NerdFont::Palette, "GTK Theme")
        .text("Select and apply a GTK theme.")
        .blank()
        .field("Current GTK theme", &theme)
        .build_string())
}

fn render_icon_theme_preview() -> Result<String> {
    let theme = get_current_icon_theme().unwrap_or_else(|_| "unknown".to_string());
    Ok(PreviewBuilder::new()
        .header(NerdFont::Image, "Icon Theme")
        .text("Select and apply a GTK icon theme.")
        .blank()
        .field("Current icon theme", &theme)
        .build_string())
}

fn render_default_app_preview(
    title: &str,
    icon: NerdFont,
    summary: &str,
    mime_types: &[&str],
) -> Result<String> {
    let mut builder = PreviewBuilder::new()
        .header(icon, title)
        .subtext(summary)
        .blank()
        .line(colors::TEAL, None, "▸ MIME Types")
        .bullets(mime_types.iter().copied())
        .blank()
        .subtext("Only apps supporting ALL formats are shown.")
        .blank()
        .line(colors::TEAL, None, "▸ Current Defaults");

    for mime in mime_types {
        let label = query_default_app(mime)
            .ok()
            .flatten()
            .map(|desktop_id| display_app_name(&desktop_id))
            .unwrap_or_else(|| "(not set)".to_string());
        builder = builder.field_indented(mime, &label);
    }

    Ok(builder.build_string())
}

fn render_disk_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(disk) = ctx.key.as_deref().filter(|k| !k.is_empty()) else {
        return Ok(String::new());
    };

    if which::which("lsblk").is_err() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::HardDrive, "Disk Overview")
            .text("lsblk not found on this system.")
            .build_string());
    }

    let warning = hex_to_ansi_fg(colors::YELLOW);
    let ok = hex_to_ansi_fg(colors::GREEN);
    let reset = "\x1b[0m";

    let mount_lines = disk_mount_status(disk);

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::HardDrive, "Disk Overview")
        .subtext("Selecting a disk will erase all data on it.")
        .blank()
        .line(colors::YELLOW, Some(NerdFont::Warning), "Mount Status");

    if mount_lines.is_empty() {
        builder = builder.raw(&format!("{ok}  No mounted partitions detected{reset}"));
    } else {
        builder = builder.raw(&format!("{warning}  Mounted partitions detected{reset}"));
        for line in &mount_lines {
            builder = builder.raw(&format!("    {line}"));
        }
        builder = builder.raw(&format!("{warning}  Unmount before proceeding.{reset}"));
    }

    let device_lines = lsblk_lines(&["-d", "-l", "-n", "-o", "NAME,SIZE,MODEL,TYPE", disk]);
    builder = builder
        .blank()
        .line(colors::TEAL, None, "Device")
        .raw_lines(&indent_lines(&device_lines, "  "))
        .blank()
        .line(colors::TEAL, None, "Partitions")
        .raw_lines(&indent_lines(
            &lsblk_lines(&["-l", "-n", "-o", "NAME,SIZE,FSTYPE,MOUNTPOINT", disk]),
            "  ",
        ));

    Ok(builder.build_string())
}

fn render_partition_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(part) = ctx.key.as_deref().filter(|k| !k.is_empty()) else {
        return Ok(String::new());
    };

    if which::which("lsblk").is_err() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Partition, "Partition Details")
            .text("lsblk not found on this system.")
            .build_string());
    }

    let overview = lsblk_lines(&["-l", "-n", "-o", "NAME,SIZE,FSTYPE,MOUNTPOINT", part]);
    let identifiers = lsblk_lines(&["-l", "-n", "-o", "NAME,UUID,PARTUUID", part]);

    let builder = PreviewBuilder::new()
        .header(NerdFont::Partition, "Partition Details")
        .subtext("Verify the filesystem and mount point before selecting.")
        .blank()
        .line(colors::TEAL, None, "Overview")
        .raw_lines(&indent_lines(&overview, "  "))
        .blank()
        .line(colors::TEAL, None, "Identifiers")
        .raw_lines(&indent_lines(&identifiers, "  "))
        .build_string();

    Ok(builder)
}

fn current_exe_command() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ins".to_string());
    shell_quote(&exe)
}

fn env_usize(name: &str) -> Option<usize> {
    env::var(name).ok().and_then(|v| v.parse::<usize>().ok())
}

fn date_in_tz(tz: &str, format: &str) -> Option<String> {
    let mut cmd = Command::new("date");
    cmd.arg(format).env("TZ", tz);
    command_output_optional(cmd)
}

fn date_local(format: &str) -> Option<String> {
    let mut cmd = Command::new("date");
    cmd.arg(format);
    command_output_optional(cmd)
}

fn format_utc_offset(raw: &str) -> String {
    if raw.len() == 5 {
        format!("UTC{}:{}", &raw[..3], &raw[3..])
    } else {
        format!("UTC{raw}")
    }
}

fn is_sway_session() -> bool {
    env::var_os("SWAYSOCK").is_some() && which::which("swaymsg").is_ok()
}

fn is_x11_session() -> bool {
    env::var_os("DISPLAY").is_some() && which::which("setxkbmap").is_ok()
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
        let active_index =
            dev.xkb_active_layout_index
                .and_then(|idx| if idx >= 0 { Some(idx as usize) } else { None });
        let active_name = dev.xkb_active_layout_name.clone();

        if let Some(ref name) = active_name {
            if seen_active.insert(name.clone()) {
                unique_active.push(name.clone());
            }
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
        if let Some(value) = extract_quoted_value(trimmed) {
            if !value.is_empty() {
                return Some(value);
            }
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

fn mime_category(mime_type: &str) -> &'static str {
    if mime_type.starts_with("image/") {
        return "Image file";
    }
    if mime_type.starts_with("video/") {
        return "Video file";
    }
    if mime_type.starts_with("audio/") {
        return "Audio file";
    }
    if mime_type.starts_with("text/") {
        return "Text document";
    }
    if mime_type == "application/pdf" {
        return "PDF document";
    }
    if mime_type.contains("zip") || mime_type.contains("tar") || mime_type.contains("rar") {
        return "Archive file";
    }
    if mime_type.contains("7z") {
        return "Archive file";
    }
    if mime_type == "application/x-appimage" {
        return "AppImage executable";
    }
    if mime_type.starts_with("application/") {
        return "Application data";
    }
    "Other"
}

fn mime_extensions(mime_type: &str) -> Vec<String> {
    let canonical = canonical_mime_type(mime_type);
    let mut entries: Vec<GlobEntry> = Vec::new();

    for path in mime_globs2_paths() {
        if let Ok(mut list) = parse_globs2(&path, &canonical) {
            entries.append(&mut list);
        }
    }

    entries.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| a.pattern.cmp(&b.pattern))
    });

    let mut seen = HashSet::new();
    let mut extensions = Vec::new();

    for entry in entries {
        let trimmed = entry.pattern.trim_start_matches('*').to_string();
        let label = if trimmed.is_empty() {
            entry.pattern
        } else {
            trimmed
        };
        if seen.insert(label.clone()) {
            extensions.push(label);
            if extensions.len() >= 8 {
                break;
            }
        }
    }

    extensions
}

fn canonical_mime_type(mime_type: &str) -> String {
    for path in mime_alias_paths() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let alias = parts.next().unwrap_or("");
                let canonical = parts.next().unwrap_or("");
                if alias == mime_type && !canonical.is_empty() {
                    return canonical.to_string();
                }
            }
        }
    }
    mime_type.to_string()
}

fn mime_alias_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/mime/aliases"));
    }
    paths.push(PathBuf::from("/usr/local/share/mime/aliases"));
    paths.push(PathBuf::from("/usr/share/mime/aliases"));
    paths.into_iter().filter(|p| p.exists()).collect()
}

fn mime_globs2_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/mime/globs2"));
    }
    paths.push(PathBuf::from("/usr/local/share/mime/globs2"));
    paths.push(PathBuf::from("/usr/share/mime/globs2"));
    paths.into_iter().filter(|p| p.exists()).collect()
}

fn parse_globs2(path: &Path, mime_type: &str) -> Result<Vec<GlobEntry>> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ':');
        let weight = parts.next().unwrap_or("0");
        let mime = parts.next().unwrap_or("");
        let pattern = parts.next().unwrap_or("");
        if mime != mime_type || pattern.is_empty() {
            continue;
        }
        let weight = weight.parse::<i32>().unwrap_or(0);
        entries.push(GlobEntry {
            weight,
            pattern: pattern.to_string(),
        });
    }

    Ok(entries)
}

fn display_app_name(desktop_id: &str) -> String {
    let info = get_application_info(desktop_id);
    info.name.unwrap_or_else(|| desktop_id.to_string())
}

fn bluetooth_connected_devices() -> Vec<String> {
    let output = Command::new("bluetoothctl")
        .args(["devices", "Connected"])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Device ") {
                let mut parts = rest.splitn(2, ' ');
                let _mac = parts.next();
                let name = parts.next().unwrap_or("").trim();
                if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                }
            } else {
                None
            }
        })
        .collect()
}

fn find_theme_variant<F>(current_theme: &str, switch_to_dark: bool, exists_fn: F) -> (String, bool)
where
    F: Fn(&str) -> bool,
{
    if switch_to_dark {
        if current_theme.ends_with("-light") {
            let base_theme = current_theme.trim_end_matches("-light");
            let dark_theme = format!("{}-dark", base_theme);
            if exists_fn(&dark_theme) {
                return (dark_theme, true);
            }
        }
        if !current_theme.ends_with("-dark") {
            let dark_theme = format!("{}-dark", current_theme);
            if exists_fn(&dark_theme) {
                return (dark_theme, true);
            }
        }
    } else if current_theme.ends_with("-dark") {
        let light_theme = current_theme.trim_end_matches("-dark");
        if exists_fn(light_theme) {
            return (light_theme.to_string(), true);
        }
        let light_theme_alt = format!("{}-light", light_theme);
        if exists_fn(&light_theme_alt) {
            return (light_theme_alt, true);
        }
    }

    (current_theme.to_string(), false)
}

fn truncate_label(label: &str, limit: usize) -> String {
    let mut chars = label.chars();
    let count = label.chars().count();
    if count <= limit {
        return label.to_string();
    }
    let mut truncated = String::new();
    for _ in 0..limit.saturating_sub(3) {
        if let Some(c) = chars.next() {
            truncated.push(c);
        }
    }
    truncated.push_str("...");
    truncated
}

fn disk_mount_status(disk: &str) -> Vec<String> {
    let lines = lsblk_lines(&["-l", "-n", "-o", "NAME,MOUNTPOINT", disk]);
    let mut mounted = Vec::new();

    for line in lines {
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap_or("");
        let mount = parts.collect::<Vec<_>>().join(" ");
        if !mount.is_empty() {
            mounted.push(format!("{name} -> {mount}"));
        }
    }

    mounted
}

fn lsblk_lines(args: &[&str]) -> Vec<String> {
    let output = Command::new("lsblk").args(args).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim_end().to_string())
        .collect()
}

fn indent_lines(lines: &[String], indent: &str) -> Vec<String> {
    if lines.is_empty() {
        return vec![format!("{indent}(unavailable)")];
    }
    lines.iter().map(|line| format!("{indent}{line}")).collect()
}

fn push_raw_lines(mut builder: PreviewBuilder, lines: &[String]) -> PreviewBuilder {
    for line in lines {
        builder = builder.raw(line);
    }
    builder
}

fn push_bullets(mut builder: PreviewBuilder, lines: &[String]) -> PreviewBuilder {
    for line in lines {
        builder = builder.bullet(line);
    }
    builder
}

fn command_output_optional(mut cmd: Command) -> Option<String> {
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
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

#[derive(Debug, Clone)]
struct GlobEntry {
    weight: i32,
    pattern: String,
}

trait PreviewBuilderExt {
    fn raw_lines(self, lines: &[String]) -> Self;
}

impl PreviewBuilderExt for PreviewBuilder {
    fn raw_lines(self, lines: &[String]) -> Self {
        push_raw_lines(self, lines)
    }
}
