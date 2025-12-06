//! Appearance settings
//!
//! Theming, animations, and wallpaper settings.

use anyhow::{Context, Result};
use std::process::Command;

use crate::common::compositor::CompositorType;
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
            .summary("Enable smooth animations and visual effects on the desktop.\n\nDisable for better performance on older hardware.\n\nOnly supported on instantwm.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);

        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::InstantWM) {
            ctx.emit_unsupported(
                "settings.appearance.animations.unsupported",
                &format!(
                    "Animation configuration is only supported on instantwm. Detected: {}. Setting saved but not applied.",
                    compositor.name()
                ),
            );
            return Ok(());
        }

        // Note: instantwmctl animated uses inverted logic:
        // 0 = animations enabled, 1 = animations disabled
        let animated_value = if enabled { "0" } else { "1" };
        let status = Command::new("instantwmctl")
            .args(["animated", animated_value])
            .status();

        match status {
            Ok(exit) if exit.success() => {
                ctx.notify("Animations", if enabled { "Enabled" } else { "Disabled" });
            }
            Ok(exit) => {
                ctx.emit_failure(
                    "settings.appearance.animations.apply_failed",
                    &format!(
                        "Failed to apply animation setting (exit code {}).",
                        exit.code().unwrap_or(-1)
                    ),
                );
            }
            Err(err) => {
                ctx.emit_failure(
                    "settings.appearance.animations.apply_error",
                    &format!("Failed to run instantwmctl: {err}"),
                );
            }
        }

        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::InstantWM) {
            return None;
        }

        let enabled = ctx.bool(Self::KEY);
        // Note: instantwmctl animated uses inverted logic:
        // 0 = animations enabled, 1 = animations disabled
        let animated_value = if enabled { "0" } else { "1" };
        let status = Command::new("instantwmctl")
            .args(["animated", animated_value])
            .status();

        let result = match status {
            Ok(exit) if exit.success() => {
                ctx.emit_info(
                    "settings.appearance.animations.restored",
                    &format!(
                        "Restored instantwm animations: {}",
                        if enabled { "enabled" } else { "disabled" }
                    ),
                );
                Ok(())
            }
            Ok(exit) => {
                ctx.emit_failure(
                    "settings.appearance.animations.restore_failed",
                    &format!(
                        "Failed to restore instantwm animations (exit code {}).",
                        exit.code().unwrap_or(-1)
                    ),
                );
                Ok(())
            }
            Err(err) => {
                ctx.emit_failure(
                    "settings.appearance.animations.restore_error",
                    &format!("Failed to run instantwmctl: {err}"),
                );
                Ok(())
            }
        };

        Some(result)
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

// ============================================================================
// GTK Icon Theme
// ============================================================================

pub struct GtkIconTheme;

impl Setting for GtkIconTheme {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.gtk_icon_theme")
            .title("Icon Theme")
            .category(Category::Appearance)
            .icon(NerdFont::Image) // Use a generic image icon or find a better one
            .summary("Select and apply a GTK icon theme.\n\nUpdates GTK 3/4 settings and GSettings for Sway.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let themes = list_icon_themes()?;
        if themes.is_empty() {
            ctx.emit_failure(
                "settings.appearance.gtk_icon_theme.no_themes",
                "No icon themes found in standard directories.",
            );
            return Ok(());
        }

        let selected = FzfWrapper::builder()
            .prompt("Select Icon Theme")
            .header("Choose an icon theme to apply globally")
            .select(themes)?;

        if let crate::menu_utils::FzfResult::Selected(theme) = selected {
            // 1. Apply to GSettings (Wayland/Sway primary)
            let status = Command::new("gsettings")
                .args(["set", "org.gnome.desktop.interface", "icon-theme", &theme])
                .status();

            match status {
                Ok(exit) if exit.success() => {
                    ctx.notify("Icon Theme", &format!("Applied '{}' to GSettings", theme));
                }
                Ok(exit) => {
                    ctx.emit_failure(
                        "settings.appearance.gtk_icon_theme.gsettings_failed",
                        &format!(
                            "GSettings failed with exit code {}",
                            exit.code().unwrap_or(-1)
                        ),
                    );
                }
                Err(e) => {
                    ctx.emit_failure(
                        "settings.appearance.gtk_icon_theme.gsettings_error",
                        &format!("Failed to execute gsettings: {e}"),
                    );
                }
            }

            // 2. Update settings.ini files for GTK 3 and 4
            if let Err(e) = update_gtk_config("3.0", "gtk-icon-theme-name", &theme) {
                ctx.emit_failure(
                    "settings.appearance.gtk_icon_theme.gtk3_error",
                    &format!("Failed to update GTK 3.0 config: {e}"),
                );
            }

            if let Err(e) = update_gtk_config("4.0", "gtk-icon-theme-name", &theme) {
                ctx.emit_failure(
                    "settings.appearance.gtk_icon_theme.gtk4_error",
                    &format!("Failed to update GTK 4.0 config: {e}"),
                );
            }
        }

        Ok(())
    }
}

inventory::submit! { &GtkIconTheme as &'static dyn Setting }

// ============================================================================
// GTK Theme
// ============================================================================

pub struct GtkTheme;

impl Setting for GtkTheme {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.gtk_theme")
            .title("Theme")
            .category(Category::Appearance)
            .icon(NerdFont::Image) // Use a generic image icon or find a better one
            .summary("Select and apply a GTK theme.\n\nUpdates GTK 3/4 settings, GSettings, and applies Libadwaita overrides (via ~/.config/gtk-4.0/).")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let themes = list_gtk_themes()?;
        if themes.is_empty() {
            ctx.emit_failure(
                "settings.appearance.gtk_theme.no_themes",
                "No GTK themes found in standard directories.",
            );
            return Ok(());
        }

        let selected = FzfWrapper::builder()
            .prompt("Select GTK Theme")
            .header("Choose a GTK theme to apply globally")
            .select(themes)?;

        if let crate::menu_utils::FzfResult::Selected(theme) = selected {
            // 1. Apply to GSettings (Wayland/Sway primary)
            let status = Command::new("gsettings")
                .args(["set", "org.gnome.desktop.interface", "gtk-theme", &theme])
                .status();

            match status {
                Ok(exit) if exit.success() => {
                    ctx.notify("GTK Theme", &format!("Applied '{}' to GSettings", theme));
                }
                Ok(exit) => {
                    ctx.emit_failure(
                        "settings.appearance.gtk_theme.gsettings_failed",
                        &format!(
                            "GSettings failed with exit code {}",
                            exit.code().unwrap_or(-1)
                        ),
                    );
                }
                Err(e) => {
                    ctx.emit_failure(
                        "settings.appearance.gtk_theme.gsettings_error",
                        &format!("Failed to execute gsettings: {e}"),
                    );
                }
            }

            // 2. Update settings.ini files for GTK 3 and 4
            if let Err(e) = update_gtk_config("3.0", "gtk-theme-name", &theme) {
                ctx.emit_failure(
                    "settings.appearance.gtk_theme.gtk3_error",
                    &format!("Failed to update GTK 3.0 config: {e}"),
                );
            }

            if let Err(e) = update_gtk_config("4.0", "gtk-theme-name", &theme) {
                ctx.emit_failure(
                    "settings.appearance.gtk_theme.gtk4_error",
                    &format!("Failed to update GTK 4.0 config: {e}"),
                );
            }

            // 3. Libadwaita/GTK4 overrides (Symlink ~/.config/gtk-4.0/gtk.css)
            if let Err(e) = apply_gtk4_overrides(&theme) {
                // Not a critical failure, but worth noting
                ctx.emit_info(
                    "settings.appearance.gtk_theme.gtk4_override_info",
                    &format!("Could not apply Libadwaita overrides (maybe theme lacks gtk-4.0 support?): {e}"),
                );
            } else {
                ctx.notify("GTK Theme", "Applied Libadwaita overrides");
            }
        }

        Ok(())
    }
}

inventory::submit! { &GtkTheme as &'static dyn Setting }

// ============================================================================
// Reset GTK Customizations
// ============================================================================

pub struct ResetGtk;

impl Setting for ResetGtk {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.reset_gtk")
            .title("Reset Customizations")
            .category(Category::Appearance)
            .icon(NerdFont::Trash)
            .summary("Reset all GTK theme and icon settings to default.\n\nRemoves custom settings.ini files and GTK4 CSS overrides.")
            .build()
    }
    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let confirmation = FzfWrapper::confirm(
            "Are you sure you want to reset all GTK theme customizations? This will clear settings.ini and remove GTK4 overrides.",
        )?;

        if matches!(confirmation, crate::menu_utils::ConfirmResult::Yes) {
            // 1. Reset GSettings
            let _ = Command::new("gsettings")
                .args(["reset", "org.gnome.desktop.interface", "gtk-theme"])
                .status();
            let _ = Command::new("gsettings")
                .args(["reset", "org.gnome.desktop.interface", "icon-theme"])
                .status();

            // 2. Remove configuration files
            if let Ok(config_dir) = dirs::config_dir().context("Could not find config directory") {
                let paths_to_remove = [
                    config_dir.join("gtk-3.0/settings.ini"),
                    config_dir.join("gtk-4.0/settings.ini"),
                    config_dir.join("gtk-4.0/gtk.css"),
                    config_dir.join("gtk-4.0/gtk-dark.css"),
                    config_dir.join("gtk-4.0/assets"),
                ];

                for path in &paths_to_remove {
                    if path.exists() {
                        if path.is_dir() && !path.is_symlink() {
                            let _ = std::fs::remove_dir_all(path);
                        } else {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
            }

            ctx.notify("GTK Reset", "GTK customizations have been cleared.");
        }

        Ok(())
    }
}

inventory::submit! { &ResetGtk as &'static dyn Setting }

// Helpers for GTK Theme

fn list_gtk_themes() -> Result<Vec<String>> {
    let mut themes = std::collections::HashSet::new();
    let dirs = [
        dirs::home_dir().map(|p| p.join(".themes")),
        dirs::home_dir().map(|p| p.join(".local/share/themes")),
        Some(std::path::PathBuf::from("/usr/share/themes")),
    ];

    for dir in dirs.into_iter().flatten() {
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                let path = entry.path();
                // Check for index.theme OR gtk-3.0/gtk.css OR gtk-4.0/gtk.css
                if path.join("index.theme").exists()
                    || path.join("gtk-3.0/gtk.css").exists()
                    || path.join("gtk-4.0/gtk.css").exists()
                {
                    if let Some(name) = entry.file_name().to_str() {
                        themes.insert(name.to_string());
                    }
                }
            }
        }
    }

    let mut result: Vec<String> = themes.into_iter().collect();
    result.sort();
    Ok(result)
}

fn apply_gtk4_overrides(theme_name: &str) -> Result<()> {
    // Find the theme directory
    let dirs = [
        dirs::home_dir().map(|p| p.join(".themes")),
        dirs::home_dir().map(|p| p.join(".local/share/themes")),
        Some(std::path::PathBuf::from("/usr/share/themes")),
    ];

    let mut theme_path = None;
    for dir in dirs.into_iter().flatten() {
        let p = dir.join(theme_name);
        if p.exists() {
            theme_path = Some(p);
            break;
        }
    }

    let theme_path = theme_path.context("Theme not found")?;
    let source_gtk4 = theme_path.join("gtk-4.0");

    if !source_gtk4.exists() {
        // Theme doesn't have explicit GTK 4 support, nothing to link
        return Err(anyhow::anyhow!("Theme has no gtk-4.0 directory"));
    }

    // Target directory: ~/.config/gtk-4.0/
    let config_dir = dirs::config_dir().context("No config dir")?.join("gtk-4.0");

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    // Items to symlink
    let items = ["gtk.css", "gtk-dark.css", "assets"];

    for item in items {
        let source = source_gtk4.join(item);
        let target = config_dir.join(item);

        // Remove existing target (file or symlink)
        if target.is_symlink() || target.exists() {
            // Use fs::remove_file for files and symlinks (even if they point to dirs)
            // Use fs::remove_dir_all if it's a real directory (not symlink)
            if target.is_dir() && !target.is_symlink() {
                std::fs::remove_dir_all(&target)?;
            } else {
                std::fs::remove_file(&target)?;
            }
        }

        if source.exists() {
            std::os::unix::fs::symlink(&source, &target)
                .with_context(|| format!("Failed to link {:?} -> {:?}", source, target))?;
        }
    }

    Ok(())
}

// Helpers for GTK Icon Theme

fn list_icon_themes() -> Result<Vec<String>> {
    let mut themes = std::collections::HashSet::new();
    let dirs = [
        dirs::home_dir().map(|p| p.join(".icons")),
        dirs::data_local_dir().map(|p| p.join("icons")),
        Some(std::path::PathBuf::from("/usr/share/icons")),
    ];

    for dir in dirs.into_iter().flatten() {
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                // Check for index.theme
                if entry.path().join("index.theme").exists() {
                    if let Some(name) = entry.file_name().to_str() {
                        themes.insert(name.to_string());
                    }
                }
            }
        }
    }

    let mut result: Vec<String> = themes.into_iter().collect();
    result.sort();
    Ok(result)
}

fn update_gtk_config(version: &str, key: &str, value: &str) -> Result<()> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join(format!("gtk-{}", version));

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    let settings_path = config_dir.join("settings.ini");
    let content = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)?
    } else {
        String::new()
    };

    let mut new_lines = Vec::new();
    let mut found_section = false;
    let mut found_key = false;
    let mut in_settings_section = false;

    // Simple parser to update INI file
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[Settings]" {
            found_section = true;
            in_settings_section = true;
            new_lines.push(line.to_string());
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_settings_section = false;
        }

        if in_settings_section && trimmed.starts_with(key) {
            new_lines.push(format!("{}={}", key, value));
            found_key = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found_section {
        if !new_lines.is_empty() && !new_lines.last().unwrap().is_empty() {
            new_lines.push("".to_string());
        }
        new_lines.push("[Settings]".to_string());
        new_lines.push(format!("{}={}", key, value));
    } else if !found_key {
        // Find where to insert the key in the [Settings] section
        // We'll just append it after the [Settings] line for simplicity in this case
        // But since we are rebuilding the list, we need to be careful.
        // Let's re-scan new_lines to find [Settings] and insert after it.
        let mut final_lines = Vec::new();
        for line in new_lines {
            final_lines.push(line.clone());
            if line.trim() == "[Settings]" && !found_key {
                final_lines.push(format!("{}={}", key, value));
                found_key = true;
            }
        }
        new_lines = final_lines;
    }

    let new_content = new_lines.join("\n");
    // Ensure trailing newline
    let final_content = if new_content.ends_with('\n') {
        new_content
    } else {
        format!("{}\n", new_content)
    };

    std::fs::write(settings_path, final_content)?;
    Ok(())
}

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
