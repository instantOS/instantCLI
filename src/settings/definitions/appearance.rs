//! Appearance settings
//!
//! Theming, animations, and wallpaper settings.

use anyhow::{Context, Result};
use std::process::Command;

use crate::common::requirements::{YAZI_PACKAGE, ZENITY_PACKAGE};
use crate::menu_utils::{FzfWrapper, MenuWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
use crate::settings::store::{BoolSettingKey, OptionalStringSettingKey};
use crate::ui::prelude::*;

// ============================================================================
// Animations
// ============================================================================

pub struct Animations;

impl Animations {
    const KEY: BoolSettingKey = BoolSettingKey::new("appearance.animations", true);
}

impl Setting for Animations {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.animations")
            .title("Animations")
            .category(Category::Appearance)
            .icon(NerdFont::Magic)
            .summary("Enable smooth animations and visual effects on the desktop.\n\nDisable for better performance on older hardware.\n\nNote: Placeholder only; changing this setting currently has no effect.")
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
            "Animations",
            if target {
                "Enabled (no effect yet)"
            } else {
                "Disabled (no effect yet)"
            },
        );
        Ok(())
    }
}

inventory::submit! { &Animations as &'static dyn Setting }

// ============================================================================
// Wallpaper Settings
// ============================================================================

pub struct SetWallpaper;

impl Setting for SetWallpaper {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper")
            .title("Wallpaper")
            .category(Category::Appearance)
            .icon(NerdFont::Image)
            .summary("Select and set a new wallpaper image.")
            .requirements(&[Requirement::Package(YAZI_PACKAGE)])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
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
                    .title("Wallpaper")
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

inventory::submit! { &SetWallpaper as &'static dyn Setting }

pub struct WallpaperLogo;

impl WallpaperLogo {
    const KEY: BoolSettingKey = BoolSettingKey::new("wallpaper.logo", true);
}

impl Setting for WallpaperLogo {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_logo")
            .title("Show Logo on Wallpaper")
            .category(Category::Appearance)
            .icon(NerdFont::Image)
            .breadcrumbs(&["Wallpaper", "Show Logo"])
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

inventory::submit! { &WallpaperLogo as &'static dyn Setting }

pub struct RandomWallpaper;

impl Setting for RandomWallpaper {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_random")
            .title("Random Wallpaper")
            .category(Category::Appearance)
            .icon(NerdFont::Refresh)
            .breadcrumbs(&["Wallpaper", "Random"])
            .summary("Fetch and set a random wallpaper from Wallhaven.\n\nRespects the 'Show Logo on Wallpaper' setting.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
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

inventory::submit! { &RandomWallpaper as &'static dyn Setting }

// Color wallpaper settings
const WALLPAPER_BG_COLOR_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("wallpaper.bg_color");
const WALLPAPER_FG_COLOR_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("wallpaper.fg_color");

pub struct WallpaperBgColor;

impl Setting for WallpaperBgColor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_bg_color")
            .title("Background Color")
            .category(Category::Appearance)
            .icon(NerdFont::Palette)
            .breadcrumbs(&["Wallpaper", "Colored", "Background"])
            .summary(
                "Choose a background color for colored wallpapers.\n\nUses zenity color picker.",
            )
            .requirements(&[Requirement::Package(ZENITY_PACKAGE)])
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

inventory::submit! { &WallpaperBgColor as &'static dyn Setting }

pub struct WallpaperFgColor;

impl Setting for WallpaperFgColor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_fg_color")
            .title("Foreground Color")
            .category(Category::Appearance)
            .icon(NerdFont::Palette)
            .breadcrumbs(&["Wallpaper", "Colored", "Foreground"])
            .summary("Choose a foreground/logo color for colored wallpapers.\n\nUses zenity color picker.")
            .requirements(&[Requirement::Package(ZENITY_PACKAGE)])
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

inventory::submit! { &WallpaperFgColor as &'static dyn Setting }

pub struct ApplyColoredWallpaper;

impl Setting for ApplyColoredWallpaper {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
                .id("appearance.wallpaper_colored")
                .title("Apply Colored Wallpaper")
                .category(Category::Appearance)
                .icon(NerdFont::Image)
                .breadcrumbs(&["Wallpaper", "Colored", "Apply"])
                .summary("Generate a solid-color wallpaper with the instantOS logo.\n\nUses the chosen background and foreground colors.")
                .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
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

inventory::submit! { &ApplyColoredWallpaper as &'static dyn Setting }

// Helper
fn pick_color_with_zenity(title: &str, initial: &str) -> Result<Option<String>> {
    let output = Command::new("zenity")
        .args(["--color-selection", "--title", title, "--color", initial])
        .output()
        .context("Failed to run zenity")?;

    if !output.status.success() {
        return Ok(None);
    }

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if let Some(hex) = rgb_to_hex(&result) {
        Ok(Some(hex))
    } else if result.starts_with('#') {
        Ok(Some(result))
    } else {
        Ok(None)
    }
}

fn rgb_to_hex(rgb: &str) -> Option<String> {
    let re = regex::Regex::new(r"rgb\((\d+),(\d+),(\d+)\)").ok()?;
    let caps = re.captures(rgb)?;
    let r: u8 = caps.get(1)?.as_str().parse().ok()?;
    let g: u8 = caps.get(2)?.as_str().parse().ok()?;
    let b: u8 = caps.get(3)?.as_str().parse().ok()?;
    Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
}
