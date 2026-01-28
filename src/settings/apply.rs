use anyhow::{Context, Result};

use crate::settings::setting;

use super::context::SettingsContext;
use super::store::SettingsStore;

pub fn run_nonpersistent_apply(debug: bool, privileged_flag: bool) -> Result<()> {
    let store = SettingsStore::load().context("loading settings file")?;
    let mut ctx = SettingsContext::new_with_notifications_disabled(store, debug, privileged_flag);

    // Restore all settings that require reapplication
    let applied = super::restore::restore_settings(&mut ctx)?;

    if applied == 0 {
        ctx.emit_info(
            "settings.apply.none",
            "No non-persistent settings are currently enabled.",
        );
    } else {
        ctx.emit_success(
            "settings.apply.completed",
            &format!(
                "Reapplied {applied} setting{}",
                if applied == 1 { "" } else { "s" }
            ),
        );
    }

    // Also apply wallpaper on supported compositors
    if let Err(e) = apply_wallpaper_if_configured() {
        if debug {
            eprintln!("Wallpaper apply skipped: {e}");
        }
    }

    Ok(())
}

fn apply_wallpaper_if_configured() -> Result<()> {
    use crate::common::compositor::CompositorType;
    use crate::wallpaper::{gnome, hyprland, sway, x11};

    let store = SettingsStore::load().context("loading settings")?;
    let path = match store.optional_string(crate::settings::store::WALLPAPER_PATH_KEY) {
        Some(p) => p,
        None => return Ok(()), // No wallpaper configured, silently skip
    };

    let compositor = CompositorType::detect();
    match compositor {
        CompositorType::Sway => sway::apply_wallpaper(&path)?,
        CompositorType::I3 | CompositorType::Dwm | CompositorType::InstantWM => {
            x11::apply_wallpaper(&path)?
        }
        CompositorType::Gnome => gnome::apply_wallpaper(&path)?,
        CompositorType::Hyprland => hyprland::apply_wallpaper(&path)?,
        _ => {} // Unsupported compositor, silently skip
    }

    Ok(())
}

pub fn run_internal_apply(
    debug: bool,
    privileged_flag: bool,
    setting_id: &str,
    _bool_value: Option<bool>,
    _string_value: Option<String>,
    settings_file: Option<std::path::PathBuf>,
) -> Result<()> {
    let store = if let Some(path) = settings_file {
        SettingsStore::load_from_path(path)
    } else {
        SettingsStore::load()
    }
    .context("loading settings file for privileged apply")?;

    // For internal apply, we should also disable notifications to be consistent
    let mut ctx = SettingsContext::new_with_notifications_disabled(store, debug, privileged_flag);

    // Find the setting by ID in the trait-based registry
    let setting = setting::setting_by_id(setting_id)
        .with_context(|| format!("unknown setting id {setting_id}"))?;

    // Apply the setting directly
    setting.apply(&mut ctx)?;
    ctx.persist()?;

    Ok(())
}
