//! Desktop-related settings actions
//!
//! Handles clipboard manager and wallpaper settings.

use anyhow::{Context, Result};
use duct::cmd;
use std::process::Command;

use crate::menu_utils::{FzfWrapper, MenuWrapper};
use crate::ui::prelude::*;

use super::super::context::SettingsContext;

pub fn apply_clipboard_manager(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let is_running = std::process::Command::new("pgrep")
        .arg("-f")
        .arg("clipmenud")
        .output()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false);

    if enabled && !is_running {
        if let Err(err) = std::process::Command::new("clipmenud")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            emit(
                Level::Warn,
                "settings.clipboard.spawn_failed",
                &format!(
                    "{} Failed to launch clipmenud: {err}",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud started");
        }
    } else if !enabled && is_running {
        if let Err(err) = cmd!("pkill", "-f", "clipmenud").run() {
            emit(
                Level::Warn,
                "settings.clipboard.stop_failed",
                &format!(
                    "{} Failed to stop clipmenud: {err}",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud stopped");
        }
    }

    Ok(())
}

pub fn pick_and_set_wallpaper(_context: &mut SettingsContext) -> Result<()> {
    // Launch file picker for images
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

pub fn set_random_wallpaper(_context: &mut SettingsContext) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    // Logo preference is read from settings inside `ins wallpaper random`
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

pub fn pick_wallpaper_bg_color(ctx: &mut SettingsContext) -> Result<()> {
    use crate::settings::store::{WALLPAPER_BG_COLOR_KEY, WALLPAPER_FG_COLOR_KEY};

    let current = ctx
        .optional_string(WALLPAPER_BG_COLOR_KEY)
        .unwrap_or_else(|| "#1a1a2e".to_string());

    if let Some(color) = pick_color_with_zenity("Background Color", &current)? {
        ctx.set_optional_string(WALLPAPER_BG_COLOR_KEY, Some(color.clone()));
        ctx.persist()?;

        // Show preview if foreground color is also set
        if let Some(fg) = ctx.optional_string(WALLPAPER_FG_COLOR_KEY) {
            FzfWrapper::builder()
                .message(format!(
                    "Background: {}\nForeground: {}\n\nUse 'Apply Colored Wallpaper' to generate.",
                    color, fg
                ))
                .title("Colors Updated")
                .show_message()?;
        } else {
            FzfWrapper::builder()
                .message(format!(
                    "Background: {}\n\nNow pick a foreground color.",
                    color
                ))
                .title("Background Set")
                .show_message()?;
        }
    }

    Ok(())
}

pub fn pick_wallpaper_fg_color(ctx: &mut SettingsContext) -> Result<()> {
    use crate::settings::store::{WALLPAPER_BG_COLOR_KEY, WALLPAPER_FG_COLOR_KEY};

    let current = ctx
        .optional_string(WALLPAPER_FG_COLOR_KEY)
        .unwrap_or_else(|| "#eaeaea".to_string());

    if let Some(color) = pick_color_with_zenity("Foreground Color", &current)? {
        ctx.set_optional_string(WALLPAPER_FG_COLOR_KEY, Some(color.clone()));
        ctx.persist()?;

        // Show preview if background color is also set
        if let Some(bg) = ctx.optional_string(WALLPAPER_BG_COLOR_KEY) {
            FzfWrapper::builder()
                .message(format!(
                    "Background: {}\nForeground: {}\n\nUse 'Apply Colored Wallpaper' to generate.",
                    bg, color
                ))
                .title("Colors Updated")
                .show_message()?;
        } else {
            FzfWrapper::builder()
                .message(format!(
                    "Foreground: {}\n\nNow pick a background color.",
                    color
                ))
                .title("Foreground Set")
                .show_message()?;
        }
    }

    Ok(())
}

pub fn apply_colored_wallpaper(_context: &mut SettingsContext) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    // Colors are read from settings inside `ins wallpaper colored`
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

/// Pick a color using zenity and return as hex
fn pick_color_with_zenity(title: &str, initial: &str) -> Result<Option<String>> {
    let output = Command::new("zenity")
        .args(["--color-selection", "--title", title, "--color", initial])
        .output()
        .context("Failed to run zenity")?;

    if !output.status.success() {
        // User cancelled
        return Ok(None);
    }

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Zenity returns rgb(r,g,b) format - convert to hex
    if let Some(hex) = rgb_to_hex(&result) {
        Ok(Some(hex))
    } else if result.starts_with('#') {
        // Already hex format (older zenity)
        Ok(Some(result))
    } else {
        Ok(None)
    }
}

/// Convert rgb(r,g,b) to #rrggbb hex format
fn rgb_to_hex(rgb: &str) -> Option<String> {
    let re = regex::Regex::new(r"rgb\((\d+),(\d+),(\d+)\)").ok()?;
    let caps = re.captures(rgb)?;
    let r: u8 = caps.get(1)?.as_str().parse().ok()?;
    let g: u8 = caps.get(2)?.as_str().parse().ok()?;
    let b: u8 = caps.get(3)?.as_str().parse().ok()?;
    Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
}
