//! Wallpaper assists.
//!
//! Thin wrappers around the native `ins wallpaper` implementation so the
//! assist menu behaves exactly like the CLI and the settings UI.

use anyhow::{Context, Result};
use std::path::Path;

use crate::assist::utils::show_notification;
use crate::menu_utils::{DialogOutcome, FilePickerBuilder};
use crate::settings::store::{SettingsStore, WALLPAPER_PATH_KEY};
use crate::wallpaper::cli::{ColoredArgs, RandomArgs, SetArgs, WallpaperCommands};
use crate::wallpaper::commands;

/// Ensure the compositor-specific backend needed to apply a wallpaper.
fn ensure_backend() -> Result<()> {
    if !commands::ensure_backend_deps()? {
        anyhow::bail!("Wallpaper backend dependencies are missing or were declined");
    }
    Ok(())
}

/// Fetch a random Wallhaven wallpaper and apply it.
///
/// Respects the "Show Logo on Wallpaper" setting.
pub fn random() -> Result<()> {
    ensure_backend()?;
    commands::run_command_blocking(WallpaperCommands::Random(RandomArgs { no_logo: false }))
}

/// Generate a solid-color wallpaper with the instantOS logo and apply it.
///
/// Uses the background and foreground colors saved in settings.
pub fn colored() -> Result<()> {
    ensure_backend()?;
    commands::run_command_blocking(WallpaperCommands::Colored(ColoredArgs {
        bg: None,
        fg: None,
    }))
}

/// Pick a custom image and set it as the wallpaper.
pub fn set_from_file() -> Result<()> {
    let path = match FilePickerBuilder::new()
        .hint("Select a wallpaper image")
        .pick_one()?
    {
        DialogOutcome::Submitted(path) => path,
        DialogOutcome::Cancelled => return Ok(()),
    };

    if !path.is_file() {
        anyhow::bail!("Selected path is not a file: {}", path.display());
    }

    ensure_backend()?;
    commands::run_command_blocking(WallpaperCommands::Set(SetArgs {
        path: path.to_string_lossy().to_string(),
    }))
}

/// Re-apply the configured wallpaper, or fetch a fresh one when it is gone.
pub fn repair() -> Result<()> {
    ensure_backend()?;

    let store = SettingsStore::load().context("loading settings")?;
    let configured = store.optional_string(WALLPAPER_PATH_KEY);
    let can_reapply = configured
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file());

    if can_reapply {
        let _ = show_notification("Wallpaper", "Re-applying wallpaper...");
        if commands::run_command_blocking(WallpaperCommands::Apply).is_ok() {
            return Ok(());
        }
        let _ = show_notification("Wallpaper", "Re-apply failed - fetching a new wallpaper...");
    } else {
        let _ = show_notification(
            "Wallpaper",
            "No usable wallpaper found - fetching a fresh one...",
        );
    }

    commands::run_command_blocking(WallpaperCommands::Random(RandomArgs { no_logo: false }))
}
