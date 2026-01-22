//! Wallpaper settings
//!
//! Set wallpapers, random wallpapers, and colored wallpapers.

use anyhow::{Context, Result};
use std::process::Command;

use crate::common::compositor::CompositorType;
use crate::common::package::{InstallResult, ensure_all};
use crate::menu_utils::{FzfWrapper, MenuWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::deps::{SWWW, YAZI, ZENITY};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{
    BoolSettingKey, OptionalStringSettingKey, SettingsStore, WALLPAPER_PATH_KEY,
};
use crate::ui::catppuccin::hex_to_ansi_bg;
use crate::ui::prelude::*;

use super::common::pick_color_with_zenity;

/// Ensure swww is installed if running on Hyprland
/// Returns Ok(true) if deps are satisfied, Ok(false) if user declined installation
fn ensure_hyprland_deps() -> Result<bool> {
    let compositor = CompositorType::detect();
    if matches!(compositor, CompositorType::Hyprland) {
        match ensure_all(&[&SWWW])? {
            InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(true),
            InstallResult::Declined
            | InstallResult::NotAvailable { .. }
            | InstallResult::Failed { .. } => Ok(false),
        }
    } else {
        Ok(true)
    }
}

const ANSI_RESET: &str = "\x1b[0m";

fn resolve_color(
    store: &SettingsStore,
    key: OptionalStringSettingKey,
    default: &str,
) -> (String, &'static str) {
    match store.optional_string(key) {
        Some(value) => (value, "Saved"),
        None => (default.to_string(), "Default"),
    }
}

fn color_swatch_block(color: &str) -> Option<String> {
    let bg = hex_to_ansi_bg(color);
    if bg.is_empty() {
        return None;
    }

    let width = 24;
    let height = 4;
    let horizontal = "-".repeat(width);
    let fill = " ".repeat(width);

    let mut lines = Vec::with_capacity(height + 2);
    lines.push(format!("+{horizontal}+"));
    for _ in 0..height {
        lines.push(format!("|{bg}{fill}{ANSI_RESET}|"));
    }
    lines.push(format!("+{horizontal}+"));
    Some(lines.join("\n"))
}

// Color wallpaper settings
const WALLPAPER_BG_COLOR_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("wallpaper.bg_color");
const WALLPAPER_FG_COLOR_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("wallpaper.fg_color");

pub struct SetWallpaper;

impl Setting for SetWallpaper {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper")
            .title("Wallpaper")
            .icon(NerdFont::Image)
            .summary("Select and set a new wallpaper image.")
            .requirements(vec![&YAZI])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        // Ensure swww is installed if on Hyprland
        if !ensure_hyprland_deps()? {
            return Ok(());
        }

        let path = MenuWrapper::file_picker()
            .hint("Select a wallpaper image")
            .pick_one()?;

        if let Some(path) = path {
            let exe = std::env::current_exe().context("Failed to get current executable path")?;
            let status = Command::new(exe)
                .args(["wallpaper", "set", &path.to_string_lossy()])
                .status()
                .context("Failed to execute wallpaper command")?;

            if status.success() {
                FzfWrapper::builder()
                    .message("Wallpaper updated successfully!")
                    .title("Wallpaper Image")
                    .show_message()?;
            } else {
                FzfWrapper::builder()
                    .message("Failed to set wallpaper.")
                    .title("Error")
                    .show_message()?;
            }
        }
        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        let compositor_name = CompositorType::detect().name();
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Image, "Wallpaper")
            .text("Select and set a new wallpaper image.")
            .blank()
            .field("Compositor", &compositor_name);

        match SettingsStore::load() {
            Ok(store) => {
                if let Some(path) = store.optional_string(WALLPAPER_PATH_KEY) {
                    let path_buf = std::path::PathBuf::from(&path);
                    let file_name = path_buf
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&path);
                    let folder = path_buf
                        .parent()
                        .and_then(|parent| parent.to_str())
                        .unwrap_or("Unknown");
                    let exists = path_buf.exists();

                    builder = builder
                        .field("Current file", file_name)
                        .field("Location", folder)
                        .field("Status", if exists { "Available" } else { "Missing" });

                    if exists && let Ok(metadata) = std::fs::metadata(&path_buf) {
                        builder = builder
                            .field("Size", &crate::arch::dualboot::format_size(metadata.len()));
                    }
                } else {
                    builder = builder
                        .field("Current file", "Not set")
                        .field("Status", "Select an image to configure");
                }
            }
            Err(_) => {
                builder = builder.field("Current file", "Unavailable");
            }
        }

        Some(builder.build_shell_script())
    }
}

pub struct WallpaperLogo;

impl WallpaperLogo {
    const KEY: BoolSettingKey = BoolSettingKey::new("wallpaper.logo", true);
}

impl Setting for WallpaperLogo {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_logo")
            .title("Show Logo on Wallpaper")
            .icon(NerdFont::Image)
            .summary("Show the instantOS logo on top of random wallpapers.\n\nWhen enabled, a logo overlay is applied when fetching random wallpapers.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let target = !current;
        ctx.set_bool(Self::KEY, target);
        ctx.notify(
            "Wallpaper Logo",
            if target {
                "Logo will be shown on random wallpapers"
            } else {
                "Logo hidden on random wallpapers"
            },
        );
        Ok(())
    }
}

pub struct RandomWallpaper;

impl Setting for RandomWallpaper {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_random")
            .title("Random Wallpaper")
            .icon(NerdFont::Refresh)
            .summary("Fetch and set a random wallpaper from Wallhaven.\n\nRespects the 'Show Logo on Wallpaper' setting.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        // Ensure swww is installed if on Hyprland
        if !ensure_hyprland_deps()? {
            return Ok(());
        }

        let exe = std::env::current_exe().context("Failed to get current executable path")?;
        let status = Command::new(exe)
            .args(["wallpaper", "random"])
            .status()
            .context("Failed to execute wallpaper random command")?;

        if status.success() {
            FzfWrapper::builder()
                .message("Random wallpaper applied!")
                .title("Wallpaper")
                .show_message()?;
        } else {
            FzfWrapper::builder()
                .message("Failed to fetch random wallpaper.")
                .title("Error")
                .show_message()?;
        }
        Ok(())
    }
}

pub struct WallpaperBgColor;

impl Setting for WallpaperBgColor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_bg_color")
            .title("Background Color")
            .icon(NerdFont::Palette)
            .summary(
                "Choose a background color for colored wallpapers.\n\nUses zenity color picker.",
            )
            .requirements(vec![&ZENITY])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx
            .optional_string(WALLPAPER_BG_COLOR_KEY)
            .unwrap_or_else(|| "#1a1a2e".to_string());
        if let Some(color) = pick_color_with_zenity("Background Color", &current)? {
            ctx.set_optional_string(WALLPAPER_BG_COLOR_KEY, Some(color.clone()));
            ctx.persist()?;
            ctx.notify("Wallpaper", &format!("Background color set to {}", color));
        }
        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Palette, "Background Color")
            .text("Choose a background color for colored wallpapers.")
            .blank();

        match SettingsStore::load() {
            Ok(store) => {
                let (value, source) = resolve_color(&store, WALLPAPER_BG_COLOR_KEY, "#1a1a2e");
                builder = builder
                    .field("Current value", &value)
                    .field("Source", source);

                if let Some(swatch) = color_swatch_block(&value) {
                    builder = builder.subtext("Preview").raw(&swatch);
                } else {
                    builder = builder.field("Preview", "Invalid color value");
                }
            }
            Err(_) => {
                builder = builder.field("Current value", "Unavailable");
            }
        }

        Some(builder.build_shell_script())
    }
}

pub struct WallpaperFgColor;

impl Setting for WallpaperFgColor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_fg_color")
            .title("Foreground Color")
            .icon(NerdFont::Palette)
            .summary("Choose a foreground/logo color for colored wallpapers.\n\nUses zenity color picker.")
            .requirements(vec![&ZENITY])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx
            .optional_string(WALLPAPER_FG_COLOR_KEY)
            .unwrap_or_else(|| "#eaeaea".to_string());
        if let Some(color) = pick_color_with_zenity("Foreground Color", &current)? {
            ctx.set_optional_string(WALLPAPER_FG_COLOR_KEY, Some(color.clone()));
            ctx.persist()?;
            ctx.notify("Wallpaper", &format!("Foreground color set to {}", color));
        }
        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Palette, "Foreground Color")
            .text("Choose a foreground/logo color for colored wallpapers.")
            .blank();

        match SettingsStore::load() {
            Ok(store) => {
                let (value, source) = resolve_color(&store, WALLPAPER_FG_COLOR_KEY, "#eaeaea");
                builder = builder
                    .field("Current value", &value)
                    .field("Source", source);

                if let Some(swatch) = color_swatch_block(&value) {
                    builder = builder.subtext("Preview").raw(&swatch);
                } else {
                    builder = builder.field("Preview", "Invalid color value");
                }
            }
            Err(_) => {
                builder = builder.field("Current value", "Unavailable");
            }
        }

        Some(builder.build_shell_script())
    }
}

pub struct ApplyColoredWallpaper;

impl Setting for ApplyColoredWallpaper {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
                .id("appearance.wallpaper_colored")
                .title("Apply Colored Wallpaper")
                .icon(NerdFont::Image)
                .summary("Generate a solid-color wallpaper with the instantOS logo.\n\nUses the chosen background and foreground colors.")
                .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        // Ensure swww is installed if on Hyprland
        if !ensure_hyprland_deps()? {
            return Ok(());
        }

        let exe = std::env::current_exe().context("Failed to get current executable path")?;
        let status = Command::new(exe)
            .args(["wallpaper", "colored"])
            .status()
            .context("Failed to execute wallpaper colored command")?;

        if status.success() {
            FzfWrapper::builder()
                .message("Colored wallpaper applied!")
                .title("Wallpaper")
                .show_message()?;
        } else {
            FzfWrapper::builder()
                .message("Failed to generate colored wallpaper.")
                .title("Error")
                .show_message()?;
        }
        Ok(())
    }
}
