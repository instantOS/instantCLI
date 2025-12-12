//! Appearance settings
//!
//! Theming, animations, and wallpaper settings.

use anyhow::{Context, Result};
use std::process::Command;

use crate::common::compositor::CompositorType;
use crate::common::instantwm::InstantWmController;
use crate::common::requirements::{YAZI_PACKAGE, ZENITY_PACKAGE};
use crate::menu_utils::{FzfWrapper, MenuWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Requirement, Setting, SettingMetadata, SettingType};
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
        self.apply_value(ctx, enabled)
    }

    fn apply_value(&self, ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
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

        let controller = InstantWmController::new();
        match controller.set_animations(enabled) {
            Ok(()) => {
                ctx.notify("Animations", if enabled { "Enabled" } else { "Disabled" });
            }
            Err(err) => {
                ctx.emit_failure(
                    "settings.appearance.animations.error",
                    &format!("Failed to apply animation setting: {err}"),
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
        Some(self.apply_value(ctx, ctx.bool(Self::KEY)))
    }
}

// ============================================================================
// Wallpaper Settings
// ============================================================================

pub struct SetWallpaper;

impl Setting for SetWallpaper {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper")
            .title("Wallpaper")
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

pub struct WallpaperFgColor;

impl Setting for WallpaperFgColor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.wallpaper_fg_color")
            .title("Foreground Color")
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

// ============================================================================
// GTK Icon Theme
// ============================================================================

pub struct GtkMenuIcons;

impl GtkMenuIcons {
    const KEY: BoolSettingKey = BoolSettingKey::new("appearance.gtk_menu_icons", false);
}

impl Setting for GtkMenuIcons {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.gtk_menu_icons")
            .title("Menu Icons")
            .icon(NerdFont::List)
            .summary("Toggle icons in GTK application menus.\n\nNote: This is a legacy setting and may not work with all modern GTK applications.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let target = !current;
        ctx.set_bool(Self::KEY, target);
        self.apply_value(ctx, target)
    }

    fn apply_value(&self, ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
        let value_str = if enabled { "true" } else { "false" };

        // Update GTK 3.0 and 4.0 settings.ini files
        if let Err(e) = update_gtk_config("3.0", "gtk-menu-images", value_str) {
            ctx.emit_failure(
                "settings.appearance.gtk_menu_images.gtk3_error",
                &format!("Failed to update GTK 3.0 config: {e}"),
            );
        }

        if let Err(e) = update_gtk_config("4.0", "gtk-menu-images", value_str) {
            ctx.emit_failure(
                "settings.appearance.gtk_menu_images.gtk4_error",
                &format!("Failed to update GTK 4.0 config: {e}"),
            );
        }

        ctx.notify("Menu Icons", if enabled { "Enabled" } else { "Disabled" });
        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        Some(self.apply_value(ctx, ctx.bool(Self::KEY)))
    }
}

pub struct GtkIconTheme;

impl Setting for GtkIconTheme {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.gtk_icon_theme")
            .title("Icon Theme")
            .icon(NerdFont::Image) // Use a generic image icon or find a better one
            .summary("Select and apply a GTK icon theme.\n\nUpdates GTK 3/4 settings and GSettings for Sway.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        use crate::settings::installable_packages::{self, GTK_ICON_THEMES};

        loop {
            let themes = list_icon_themes()?;

            // Build options list with "Install more..." at top
            let mut options: Vec<String> = Vec::new();
            let install_more_key = format!("{} Install more icon themes...", NerdFont::Package);

            options.push(install_more_key.to_string());

            // Add separator if we have themes
            if !themes.is_empty() {
                options.push("─────────────────────".to_string());
            }

            // Add all theme names
            for theme in &themes {
                options.push(theme.clone());
            }

            if themes.is_empty() {
                ctx.emit_info(
                    "settings.appearance.gtk_icon_theme.no_themes",
                    "No icon themes found. Select 'Install more icon themes...' to install one.",
                );
            }

            let selected = FzfWrapper::builder()
                .prompt("Select Icon Theme")
                .header("Choose an icon theme to apply globally")
                .select(options)?;

            match selected {
                crate::menu_utils::FzfResult::Selected(selection) => {
                    if selection == install_more_key {
                        // Show install more menu
                        let installed = installable_packages::show_install_more_menu(
                            "GTK Icon Theme",
                            GTK_ICON_THEMES,
                        )?;
                        if installed {
                            // Loop back to show updated theme list
                            continue;
                        }
                        // User cancelled or nothing installed, loop back
                        continue;
                    } else if selection.starts_with('─') {
                        // Separator selected, ignore and loop back
                        continue;
                    } else {
                        // User selected a theme
                        apply_icon_theme_changes(ctx, &selection);
                        return Ok(());
                    }
                }
                crate::menu_utils::FzfResult::MultiSelected(_)
                | crate::menu_utils::FzfResult::Error(_) => {
                    // Multi-selection or error, just loop back
                    continue;
                }
                crate::menu_utils::FzfResult::Cancelled => {
                    return Ok(());
                }
            }
        }
    }
}

// ============================================================================
// GTK Theme
// ============================================================================

pub struct GtkTheme;

impl Setting for GtkTheme {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.gtk_theme")
            .title("Theme")
            .icon(NerdFont::Image) // Use a generic image icon or find a better one
            .summary("Select and apply a GTK theme.\n\nUpdates GTK 3/4 settings, GSettings, and applies Libadwaita overrides (via ~/.config/gtk-4.0/).")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        use crate::settings::installable_packages::{self, GTK_THEMES};

        loop {
            let themes = list_gtk_themes()?;

            // Build options list with "Install more..." at top
            let mut options: Vec<String> = Vec::new();
            let install_more_key = format!("{} Install more themes...", NerdFont::Package);

            options.push(install_more_key.to_string());

            // Add separator if we have themes
            if !themes.is_empty() {
                options.push("─────────────────────".to_string());
            }

            // Add all theme names
            for theme in &themes {
                options.push(theme.clone());
            }

            if themes.is_empty() {
                ctx.emit_info(
                    "settings.appearance.gtk_theme.no_themes",
                    "No GTK themes found. Select 'Install more themes...' to install one.",
                );
            }

            let selected = FzfWrapper::builder()
                .prompt("Select GTK Theme")
                .header("Choose a GTK theme to apply globally")
                .select(options)?;

            match selected {
                crate::menu_utils::FzfResult::Selected(selection) => {
                    if selection == install_more_key {
                        // Show install more menu
                        let installed =
                            installable_packages::show_install_more_menu("GTK Theme", GTK_THEMES)?;
                        if installed {
                            // Loop back to show updated theme list
                            continue;
                        }
                        // User cancelled or nothing installed, loop back
                        continue;
                    } else if selection.starts_with('─') {
                        // Separator selected, ignore and loop back
                        continue;
                    } else {
                        // User selected a theme
                        apply_gtk_theme_changes(ctx, &selection);
                        return Ok(());
                    }
                }
                _ => {
                    return Ok(());
                }
            }
        }
    }
}

// ============================================================================
// Reset GTK Customizations
// ============================================================================

pub struct ResetGtk;

impl Setting for ResetGtk {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.reset_gtk")
            .title("Reset Customizations")
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
                    // Check symlink first - exists() returns false for broken symlinks
                    if path.is_symlink() || path.exists() {
                        if path.is_dir() && !path.is_symlink() {
                            let _ = std::fs::remove_dir_all(path);
                        } else {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }

                // Also remove the gtk-4.0 directory itself if it's a symlink (e.g., from dotfiles)
                let gtk4_dir = config_dir.join("gtk-4.0");
                if gtk4_dir.is_symlink() {
                    let _ = std::fs::remove_file(&gtk4_dir);
                }
            }

            ctx.notify("GTK Reset", "GTK customizations have been cleared.");
        }

        Ok(())
    }
}

// ============================================================================
// Reset Qt Customizations
// ============================================================================

pub struct ResetQt;

impl Setting for ResetQt {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.reset_qt")
            .title("Reset Customizations")
            .icon(NerdFont::Trash)
            .summary("Reset all Qt theme settings to default.\n\nRemoves qt5ct, qt6ct, and Kvantum configuration directories.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let confirmation = FzfWrapper::confirm(
            "Are you sure you want to reset all Qt theme customizations? This will remove qt5ct, qt6ct, and Kvantum configurations.",
        )?;

        if matches!(confirmation, crate::menu_utils::ConfirmResult::Yes)
            && let Ok(config_dir) = dirs::config_dir().context("Could not find config directory")
        {
            let dirs_to_remove = [
                config_dir.join("qt5ct"),
                config_dir.join("qt6ct"),
                config_dir.join("Kvantum"),
            ];

            let mut removed_count = 0;
            for dir in &dirs_to_remove {
                if dir.exists()
                    && let Ok(()) = std::fs::remove_dir_all(dir)
                {
                    removed_count += 1;
                }
            }

            if removed_count > 0 {
                ctx.notify(
                    "Qt Reset",
                    &format!(
                        "Removed {} Qt configuration {}. Restart Qt applications to see changes.",
                        removed_count,
                        if removed_count == 1 {
                            "directory"
                        } else {
                            "directories"
                        }
                    ),
                );
            } else {
                ctx.notify(
                    "Qt Reset",
                    "No Qt configuration directories found to remove.",
                );
            }
        }

        Ok(())
    }
}

fn apply_gtk_theme_changes(ctx: &mut SettingsContext, theme: &str) {
    // 1. Apply to GSettings (Wayland/Sway primary)
    let status = Command::new("gsettings")
        .args(["set", "org.gnome.desktop.interface", "gtk-theme", theme])
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
    if let Err(e) = update_gtk_config("3.0", "gtk-theme-name", theme) {
        ctx.emit_failure(
            "settings.appearance.gtk_theme.gtk3_error",
            &format!("Failed to update GTK 3.0 config: {e}"),
        );
    }

    if let Err(e) = update_gtk_config("4.0", "gtk-theme-name", theme) {
        ctx.emit_failure(
            "settings.appearance.gtk_theme.gtk4_error",
            &format!("Failed to update GTK 4.0 config: {e}"),
        );
    }

    // 3. Libadwaita/GTK4 overrides (Symlink ~/.config/gtk-4.0/gtk.css)
    if let Err(e) = apply_gtk4_overrides(theme) {
        // Not a critical failure, but worth noting
        ctx.emit_info(
            "settings.appearance.gtk_theme.gtk4_override_info",
            &format!(
                "Could not apply Libadwaita overrides (maybe theme lacks gtk-4.0 support?): {e}"
            ),
        );
    } else {
        ctx.notify("GTK Theme", "Applied Libadwaita overrides");
    }
}

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
                if (path.join("index.theme").exists()
                    || path.join("gtk-3.0/gtk.css").exists()
                    || path.join("gtk-4.0/gtk.css").exists())
                    && let Some(name) = entry.file_name().to_str()
                {
                    themes.insert(name.to_string());
                }
            }
        }
    }

    let mut result: Vec<String> = themes.into_iter().collect();
    result.sort();
    Ok(result)
}

/// Check if a theme with the given name exists
fn theme_exists(theme_name: &str) -> bool {
    let dirs = [
        dirs::home_dir().map(|p| p.join(".themes")),
        dirs::home_dir().map(|p| p.join(".local/share/themes")),
        Some(std::path::PathBuf::from("/usr/share/themes")),
    ];

    for dir in dirs.into_iter().flatten() {
        let theme_path = dir.join(theme_name);
        if theme_path.exists()
            && (theme_path.join("index.theme").exists()
                || theme_path.join("gtk-3.0/gtk.css").exists()
                || theme_path.join("gtk-4.0/gtk.css").exists())
        {
            return true;
        }
    }
    false
}

/// Get the current GTK theme name
fn get_current_gtk_theme() -> Result<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
        .context("Failed to query current GTK theme")?;

    let theme = String::from_utf8_lossy(&output.stdout);
    // Remove quotes and whitespace
    Ok(theme
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string())
}

/// Set the GTK theme
fn set_gtk_theme(theme_name: &str) -> Result<()> {
    let status = Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.interface",
            "gtk-theme",
            theme_name,
        ])
        .status()
        .context("Failed to set GTK theme")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Failed to set GTK theme to {}", theme_name);
    }
}

/// Check if an icon theme with the given name exists
fn icon_theme_exists(theme_name: &str) -> bool {
    let dirs = [
        dirs::home_dir().map(|p| p.join(".icons")),
        dirs::home_dir().map(|p| p.join(".local/share/icons")),
        Some(std::path::PathBuf::from("/usr/share/icons")),
    ];

    for dir in dirs.into_iter().flatten() {
        let theme_path = dir.join(theme_name);
        if theme_path.exists() && theme_path.join("index.theme").exists() {
            return true;
        }
    }
    false
}

/// Get the current icon theme name
fn get_current_icon_theme() -> Result<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .context("Failed to query current icon theme")?;

    let theme = String::from_utf8_lossy(&output.stdout);
    // Remove quotes and whitespace
    Ok(theme
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string())
}

/// Set the icon theme
fn set_icon_theme(theme_name: &str) -> Result<()> {
    let status = Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.interface",
            "icon-theme",
            theme_name,
        ])
        .status()
        .context("Failed to set icon theme")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Failed to set icon theme to {}", theme_name);
    }
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
        // Theme doesn't have explicit GTK 4 support - clear any existing overrides
        // so that GTK 4 apps fall back to their default appearance
        let config_dir = dirs::config_dir().context("No config dir")?.join("gtk-4.0");
        if config_dir.exists() {
            let items = ["gtk.css", "gtk-dark.css", "assets"];
            for item in items {
                let target = config_dir.join(item);
                if target.is_symlink() || target.exists() {
                    if target.is_dir() && !target.is_symlink() {
                        let _ = std::fs::remove_dir_all(&target);
                    } else {
                        let _ = std::fs::remove_file(&target);
                    }
                }
            }
        }
        return Err(anyhow::anyhow!("Theme has no gtk-4.0 directory"));
    }

    // Target directory: ~/.config/gtk-4.0/
    let config_dir = dirs::config_dir().context("No config dir")?.join("gtk-4.0");

    // Handle broken symlinks: is_symlink() returns true even for broken symlinks,
    // but exists() returns false. Remove broken symlinks before creating directory.
    if config_dir.is_symlink() && !config_dir.exists() {
        std::fs::remove_file(&config_dir)?;
    }

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

fn apply_icon_theme_changes(ctx: &mut SettingsContext, theme: &str) {
    // 1. Apply to GSettings (Wayland/Sway primary)
    let status = Command::new("gsettings")
        .args(["set", "org.gnome.desktop.interface", "icon-theme", theme])
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
    if let Err(e) = update_gtk_config("3.0", "gtk-icon-theme-name", theme) {
        ctx.emit_failure(
            "settings.appearance.gtk_icon_theme.gtk3_error",
            &format!("Failed to update GTK 3.0 config: {e}"),
        );
    }

    if let Err(e) = update_gtk_config("4.0", "gtk-icon-theme-name", theme) {
        ctx.emit_failure(
            "settings.appearance.gtk_icon_theme.gtk4_error",
            &format!("Failed to update GTK 4.0 config: {e}"),
        );
    }
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
                if entry.path().join("index.theme").exists()
                    && let Some(name) = entry.file_name().to_str()
                {
                    themes.insert(name.to_string());
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

// ============================================================================
// Dark Mode
// ============================================================================

/// Find the opposite theme variant (dark ↔ light) for a given theme name.
///
/// Returns `(new_theme_name, changed)` where `changed` indicates if a variant was found.
fn find_theme_variant<F>(current_theme: &str, switch_to_dark: bool, exists_fn: F) -> (String, bool)
where
    F: Fn(&str) -> bool,
{
    if switch_to_dark {
        // Currently light, switch to dark
        if current_theme.ends_with("-light") {
            // Try base-dark variant first
            let base_theme = current_theme.trim_end_matches("-light");
            let dark_theme = format!("{}-dark", base_theme);
            if exists_fn(&dark_theme) {
                return (dark_theme, true);
            }
        }
        // Check if -dark variant exists for the current theme
        if !current_theme.ends_with("-dark") {
            let dark_theme = format!("{}-dark", current_theme);
            if exists_fn(&dark_theme) {
                return (dark_theme, true);
            }
        }
    } else {
        // Currently dark, switch to light
        if current_theme.ends_with("-dark") {
            // Try removing -dark suffix
            let light_theme = current_theme.trim_end_matches("-dark");
            if exists_fn(light_theme) {
                return (light_theme.to_string(), true);
            }
            // Try explicit -light variant
            let light_theme_alt = format!("{}-light", light_theme);
            if exists_fn(&light_theme_alt) {
                return (light_theme_alt, true);
            }
        }
    }

    // No variant found, keep current theme
    (current_theme.to_string(), false)
}

/// Check if the system is currently in dark mode by querying gsettings color-scheme.
fn is_dark_mode() -> Result<bool> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .context("Failed to query gsettings color-scheme")?;

    Ok(String::from_utf8_lossy(&output.stdout).contains("prefer-dark"))
}

/// Set the gsettings color-scheme preference.
fn set_color_scheme(prefer_dark: bool) -> Result<()> {
    let scheme = if prefer_dark {
        "prefer-dark"
    } else {
        "default"
    };
    let status = Command::new("gsettings")
        .args(["set", "org.gnome.desktop.interface", "color-scheme", scheme])
        .status()
        .context("Failed to set gsettings color-scheme")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("gsettings returned non-zero exit code")
    }
}

pub struct DarkMode;

impl Setting for DarkMode {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.dark_mode")
            .title("Dark Mode")
            .icon(NerdFont::Moon)
            .summary("Request applications to use dark theme.\n\nSwitches between GTK and icon theme variants when available.\nSets color-scheme preference for GTK 4+ and Libadwaita apps.\nChanges apply instantly to running applications.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current_gtk_theme =
            get_current_gtk_theme().context("Failed to get current GTK theme")?;
        let current_icon_theme =
            get_current_icon_theme().context("Failed to get current icon theme")?;
        let is_dark = is_dark_mode()?;

        // Find theme variants (dark ↔ light)
        let switch_to_dark = !is_dark;
        let (new_gtk_theme, gtk_changed) =
            find_theme_variant(&current_gtk_theme, switch_to_dark, theme_exists);
        let (new_icon_theme, icon_changed) =
            find_theme_variant(&current_icon_theme, switch_to_dark, icon_theme_exists);

        // Apply theme changes
        if gtk_changed {
            set_gtk_theme(&new_gtk_theme).context("Failed to set GTK theme")?;
        }
        if icon_changed {
            set_icon_theme(&new_icon_theme).context("Failed to set icon theme")?;
        }

        // Set color-scheme for GTK 4+ compatibility
        set_color_scheme(switch_to_dark)?;

        // Build notification
        let mut details = vec![];
        if gtk_changed {
            details.push(format!("GTK: {} → {}", current_gtk_theme, new_gtk_theme));
        }
        if icon_changed {
            details.push(format!(
                "Icons: {} → {}",
                current_icon_theme, new_icon_theme
            ));
        }

        let status_text = if switch_to_dark {
            "Enabled"
        } else {
            "Disabled"
        };
        let message = if details.is_empty() {
            status_text.to_string()
        } else {
            format!("{}\n{}", status_text, details.join("\n"))
        };

        ctx.notify("Dark Mode", &message);
        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        // Shell command that FZF runs lazily when this item is focused
        Some(
            r#"bash -c '
scheme=$(gsettings get org.gnome.desktop.interface color-scheme 2>/dev/null)
gtk_theme=$(gsettings get org.gnome.desktop.interface gtk-theme 2>/dev/null)
icon_theme=$(gsettings get org.gnome.desktop.interface icon-theme 2>/dev/null)

if echo "$scheme" | grep -q "prefer-dark"; then
    status="Dark"
else
    status="Light"
fi

echo "Toggle between light and dark theme variants."
echo ""
echo "Switches between GTK and icon theme variants:"
echo "  GTK: Pop ↔ Pop-dark"
echo "  Icons: Papirus ↔ Papirus-Dark"
echo "and sets color-scheme preference for GTK 4+ compatibility."
echo "Changes apply instantly to running GTK applications."
echo ""
echo "Current GTK theme: $gtk_theme"
echo "Current icon theme: $icon_theme"
echo "Current mode: $status"
'"#
            .to_string(),
        )
    }
}
