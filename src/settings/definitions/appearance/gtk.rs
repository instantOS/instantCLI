//! GTK theme and icon settings
//!
//! Configure GTK themes, icon themes, and menu icons.

use anyhow::{Context, Result};
use std::process::Command;

use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuItem};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::installable_packages::{self, GTK_ICON_THEMES, GTK_THEMES};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{BoolSettingKey, GTK_ICON_THEME_KEY, GTK_THEME_KEY, StringSettingKey};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::prelude::*;

use super::common::{apply_gtk4_overrides, list_gtk_themes, list_icon_themes, update_gtk_config};

// ============================================================================
// GTK Menu Icons
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

// ============================================================================
// Theme Menu Items
// ============================================================================

#[derive(Clone)]
struct ThemeMenuItem {
    kind: ThemeMenuItemKind,
    preview_title: &'static str,
    preview_icon: NerdFont,
}

#[derive(Clone)]
enum ThemeMenuItemKind {
    InstallMore { label: String },
    Theme { name: String, is_current: bool },
}

impl ThemeMenuItem {
    fn install_more(label: String, preview_title: &'static str, preview_icon: NerdFont) -> Self {
        Self {
            kind: ThemeMenuItemKind::InstallMore { label },
            preview_title,
            preview_icon,
        }
    }

    fn theme(
        name: String,
        is_current: bool,
        preview_title: &'static str,
        preview_icon: NerdFont,
    ) -> Self {
        Self {
            kind: ThemeMenuItemKind::Theme { name, is_current },
            preview_title,
            preview_icon,
        }
    }

    fn is_current(&self) -> bool {
        if let ThemeMenuItemKind::Theme { is_current, .. } = &self.kind {
            *is_current
        } else {
            false
        }
    }
}

impl FzfSelectable for ThemeMenuItem {
    fn fzf_display_text(&self) -> String {
        match &self.kind {
            ThemeMenuItemKind::InstallMore { label } => format!(
                "{} {}",
                format_icon_colored(NerdFont::Package, colors::SAPPHIRE),
                label
            ),
            ThemeMenuItemKind::Theme { name, is_current } => {
                let icon = if *is_current {
                    NerdFont::CheckSquare
                } else {
                    NerdFont::Square
                };
                let color = if *is_current {
                    colors::GREEN
                } else {
                    colors::OVERLAY1
                };
                format!("{} {}", format_icon_colored(icon, color), name)
            }
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match &self.kind {
            ThemeMenuItemKind::Theme { name, is_current } => PreviewBuilder::new()
                .header(self.preview_icon, self.preview_title)
                .field("Theme", name)
                .field(
                    "Status",
                    if *is_current {
                        "Currently applied"
                    } else {
                        "Available"
                    },
                )
                .build(),
            ThemeMenuItemKind::InstallMore { .. } => PreviewBuilder::new()
                .header(self.preview_icon, self.preview_title)
                .text("Install additional themes and return to this list.")
                .build(),
        }
    }

    fn fzf_key(&self) -> String {
        match &self.kind {
            ThemeMenuItemKind::InstallMore { .. } => "__install_more__".to_string(),
            ThemeMenuItemKind::Theme { name, .. } => format!("theme:{name}"),
        }
    }
}

// ============================================================================
// GTK Icon Theme
// ============================================================================

pub struct GtkIconTheme;

impl GtkIconTheme {
    const KEY: StringSettingKey = GTK_ICON_THEME_KEY;
}

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
        SettingType::Choice { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        loop {
            let themes = list_icon_themes()?;
            let current = ctx.string(Self::KEY);

            // Build options list with "Install more..." at top
            let mut options: Vec<MenuItem<ThemeMenuItem>> = Vec::new();

            options.push(MenuItem::entry(ThemeMenuItem::install_more(
                "Install more icon themes...".to_string(),
                "Icon Theme",
                NerdFont::Image,
            )));

            // Add separator if we have themes
            if !themes.is_empty() {
                options.push(MenuItem::line());
            }

            // Add all theme names
            for theme in &themes {
                options.push(MenuItem::entry(ThemeMenuItem::theme(
                    theme.clone(),
                    theme == &current,
                    "Icon Theme",
                    NerdFont::Image,
                )));
            }

            if themes.is_empty() {
                ctx.emit_info(
                    "settings.appearance.gtk_icon_theme.no_themes",
                    "No icon themes found. Select 'Install more icon themes...' to install one.",
                );
            }

            let mut builder = FzfWrapper::builder()
                .prompt("Select Icon Theme")
                .header(Header::fancy("Choose an icon theme to apply globally"))
                .args(fzf_mocha_args())
                .responsive_layout();

            if let Some(index) = options.iter().position(|m| {
                if let MenuItem::Entry(item) = m {
                    item.is_current()
                } else {
                    false
                }
            }) {
                builder = builder.initial_index(index);
            }

            let selected = builder.select_menu(options)?;

            match selected {
                FzfResult::Selected(selection) => match selection.kind {
                    ThemeMenuItemKind::InstallMore { .. } => {
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
                    }
                    ThemeMenuItemKind::Theme { name, .. } => {
                        // User selected a theme
                        apply_icon_theme_changes(ctx, &name);
                        ctx.set_string(Self::KEY, &name);
                        let _ = ctx.refresh_string_source(Self::KEY);
                        return Ok(());
                    }
                },
                _ => {
                    return Ok(());
                }
            }
        }
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::IconTheme))
    }
}

// ============================================================================
// GTK Theme
// ============================================================================

pub struct GtkTheme;

impl GtkTheme {
    const KEY: StringSettingKey = GTK_THEME_KEY;
}

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
        SettingType::Choice { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        loop {
            let themes = list_gtk_themes()?;
            let current = ctx.string(Self::KEY);

            // Build options list with "Install more..." at top
            let mut options: Vec<MenuItem<ThemeMenuItem>> = Vec::new();

            options.push(MenuItem::entry(ThemeMenuItem::install_more(
                "Install more themes...".to_string(),
                "GTK Theme",
                NerdFont::Palette,
            )));

            // Add separator if we have themes
            if !themes.is_empty() {
                options.push(MenuItem::line());
            }

            // Add all theme names
            for theme in &themes {
                options.push(MenuItem::entry(ThemeMenuItem::theme(
                    theme.clone(),
                    theme == &current,
                    "GTK Theme",
                    NerdFont::Palette,
                )));
            }

            if themes.is_empty() {
                ctx.emit_info(
                    "settings.appearance.gtk_theme.no_themes",
                    "No GTK themes found. Select 'Install more themes...' to install one.",
                );
            }

            let mut builder = FzfWrapper::builder()
                .prompt("Select GTK Theme")
                .header(Header::fancy("Choose a GTK theme to apply globally"))
                .args(fzf_mocha_args())
                .responsive_layout();

            if let Some(index) = options.iter().position(|m| {
                if let MenuItem::Entry(item) = m {
                    item.is_current()
                } else {
                    false
                }
            }) {
                builder = builder.initial_index(index);
            }

            let selected = builder.select_menu(options)?;

            match selected {
                FzfResult::Selected(selection) => match selection.kind {
                    ThemeMenuItemKind::InstallMore { .. } => {
                        // Show install more menu
                        let installed =
                            installable_packages::show_install_more_menu("GTK Theme", GTK_THEMES)?;
                        if installed {
                            // Loop back to show updated theme list
                            continue;
                        }
                        // User cancelled or nothing installed, loop back
                        continue;
                    }
                    ThemeMenuItemKind::Theme { name, .. } => {
                        // User selected a theme
                        apply_gtk_theme_changes(ctx, &name);
                        ctx.set_string(Self::KEY, &name);
                        let _ = ctx.refresh_string_source(Self::KEY);
                        return Ok(());
                    }
                },
                _ => {
                    return Ok(());
                }
            }
        }
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::GtkTheme))
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
            let _ = Command::new("timeout")
                .args([
                    "10s",
                    "gsettings",
                    "reset",
                    "org.gnome.desktop.interface",
                    "gtk-theme",
                ])
                .status();
            let _ = Command::new("timeout")
                .args([
                    "10s",
                    "gsettings",
                    "reset",
                    "org.gnome.desktop.interface",
                    "icon-theme",
                ])
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
// Helper Functions
// ============================================================================

fn apply_gtk_theme_changes(ctx: &mut SettingsContext, theme: &str) {
    // 1. Apply to GSettings (Wayland/Sway primary)
    let status = Command::new("timeout")
        .args([
            "10s",
            "gsettings",
            "set",
            "org.gnome.desktop.interface",
            "gtk-theme",
            theme,
        ])
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

fn apply_icon_theme_changes(ctx: &mut SettingsContext, theme: &str) {
    // 1. Apply to GSettings (Wayland/Sway primary)
    let status = Command::new("timeout")
        .args([
            "10s",
            "gsettings",
            "set",
            "org.gnome.desktop.interface",
            "icon-theme",
            theme,
        ])
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
