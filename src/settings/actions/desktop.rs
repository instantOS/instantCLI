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
        if let Err(err) = std::process::Command::new("clipmenud").spawn() {
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
    // Load wallpaper config to check logo preference
    let config =
        crate::wallpaper::config::WallpaperConfig::load().context("Failed to load config")?;

    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    let mut args = vec!["wallpaper", "random"];
    if !config.show_logo {
        args.push("--no-logo");
    }

    let status = Command::new(exe)
        .args(&args)
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
