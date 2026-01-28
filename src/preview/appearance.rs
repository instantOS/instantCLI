use anyhow::Result;
use std::process::Command;

use crate::settings::definitions::appearance::common::{
    get_current_gtk_theme, get_current_icon_theme, icon_theme_exists, is_dark_mode, theme_exists,
};
use crate::ui::catppuccin::colors;
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_dark_mode_preview() -> Result<String> {
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

    let arrow = char::from(NerdFont::ArrowPointer);
    if gtk_changed {
        builder = builder.raw(&format!("  GTK: {gtk_theme} {arrow} {new_gtk}"));
    }
    if icon_changed {
        builder = builder.raw(&format!("  Icons: {icon_theme} {arrow} {new_icon}"));
    }

    Ok(builder.build_string())
}

pub(crate) fn render_gtk_theme_preview() -> Result<String> {
    let theme = get_current_gtk_theme().unwrap_or_else(|_| "unknown".to_string());
    Ok(PreviewBuilder::new()
        .header(NerdFont::Palette, "GTK Theme")
        .text("Select and apply a GTK theme.")
        .blank()
        .field("Current GTK theme", &theme)
        .build_string())
}

pub(crate) fn render_icon_theme_preview() -> Result<String> {
    let theme = get_current_icon_theme().unwrap_or_else(|_| "unknown".to_string());
    Ok(PreviewBuilder::new()
        .header(NerdFont::Image, "Icon Theme")
        .text("Select and apply a GTK icon theme.")
        .blank()
        .field("Current icon theme", &theme)
        .build_string())
}

pub(crate) fn render_cursor_theme_preview() -> Result<String> {
    let theme = get_current_cursor_theme().unwrap_or_else(|| "unknown".to_string());
    Ok(PreviewBuilder::new()
        .header(NerdFont::Mouse, "Cursor Theme")
        .text("Select and apply a cursor theme for Sway.")
        .text("Updates gsettings cursor-theme setting.")
        .text("Only supported on Sway.")
        .blank()
        .field("Current cursor theme", &theme)
        .build_string())
}

fn get_current_cursor_theme() -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "cursor-theme"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let theme = String::from_utf8_lossy(&output.stdout);
    Some(
        theme
            .trim()
            .trim_matches('\'')
            .trim_matches('"')
            .to_string(),
    )
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
