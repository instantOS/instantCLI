//! Dark mode setting
//!
//! Toggle between light and dark theme variants.

use anyhow::{Context, Result};

use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

use super::common::{
    get_current_gtk_theme, get_current_icon_theme, icon_theme_exists, is_dark_mode,
    set_color_scheme, set_gtk_theme, set_icon_theme, theme_exists,
};

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
        // Using timeout(1) to prevent gsettings DBus hangs from blocking fzf
        Some(
            r#"bash -c '
scheme=$(timeout 1s gsettings get org.gnome.desktop.interface color-scheme 2>/dev/null || echo "default")
gtk_theme=$(timeout 1s gsettings get org.gnome.desktop.interface gtk-theme 2>/dev/null || echo "unknown")
icon_theme=$(timeout 1s gsettings get org.gnome.desktop.interface icon-theme 2>/dev/null || echo "unknown")

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

// ============================================================================
// Helper Functions
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
