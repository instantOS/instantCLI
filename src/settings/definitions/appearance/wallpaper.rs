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
use crate::settings::store::{BoolSettingKey, OptionalStringSettingKey};
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
