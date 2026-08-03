//! Setup command implementations
//!
//! Handles the `ins setup` command and its subcommands.

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::common::compositor::CompositorType;
use crate::common::compositor::config::{WindowManager, WmConfigManager};
use crate::common::instantwmctl;
use crate::ui::prelude::*;

#[derive(Subcommand, Debug, Clone)]
pub enum SetupCommands {
    /// Set up Sway window manager integration
    ///
    /// This command:
    /// - Exports assist keybinds to the shared config file
    /// - Configures cursor theme
    /// - Adds an include to your main sway config
    /// - Reloads Sway to apply changes
    Sway,

    /// Set up i3 window manager integration
    ///
    /// This command:
    /// - Exports assist keybinds to the shared config file
    /// - Adds an include to your main i3 config
    /// - Reloads i3 to apply changes
    I3,

    /// Set up instantWM window manager integration
    ///
    /// This command:
    /// - Exports assist keybinds to ~/.config/instantwm/assist.toml
    /// - Adds an include to your config.toml if not present
    /// - Reloads instantWM to apply changes
    InstantWM,

    /// Set up niri compositor integration
    ///
    /// This command:
    /// - Ensures ~/.config/niri/instant.kdl exists (managed by instantCLI)
    /// - Adds an `include "instant.kdl"` line to your main niri config
    /// - Reloads niri to apply changes
    Niri,
}

/// Handle setup command dispatch
pub fn handle_setup_command(command: SetupCommands) -> Result<()> {
    let wm = match command {
        SetupCommands::Sway => WindowManager::Sway,
        SetupCommands::I3 => WindowManager::I3,
        SetupCommands::InstantWM => WindowManager::InstantWM,
        SetupCommands::Niri => WindowManager::Niri,
    };

    match &command {
        SetupCommands::InstantWM => setup_instantwm(),
        SetupCommands::Niri => setup_niri(),
        _ => setup_wm(wm),
    }
}

fn setup_wm(wm: WindowManager) -> Result<()> {
    let compositor = validate_compositor(&wm);
    let manager = WmConfigManager::new(wm);
    let config_changed = write_config_if_changed(&manager)?;
    let include_added = ensure_main_config_include(&manager, &wm)?;
    report_status(&wm, config_changed, include_added, &manager);
    if config_changed || include_added {
        maybe_reload_wm(&manager, &wm, &compositor);
    }
    Ok(())
}

fn setup_instantwm() -> Result<()> {
    let wm = WindowManager::InstantWM;
    let compositor = validate_compositor(&wm);
    let manager = WmConfigManager::new(wm);
    ensure_main_config_exists(&manager)?;
    let config_changed = write_instantwm_config_if_changed(&manager)?;
    let include_added = ensure_main_config_include(&manager, &wm)?;
    report_status(&wm, config_changed, include_added, &manager);
    if config_changed || include_added {
        maybe_reload_wm(&manager, &wm, &compositor);
    }
    Ok(())
}

/// Set up niri integration.
///
/// Unlike sway/i3 we do not generate a static `instant.kdl` here. The file is
/// populated dynamically by `ins settings ...` calls (mouse speed, accel
/// profile, keyboard layout). This command's job is to (a) make sure the file
/// exists with a header and (b) ensure `include "instant.kdl"` is present in
/// the main niri config so live edits to `instant.kdl` actually take effect.
fn setup_niri() -> Result<()> {
    let wm = WindowManager::Niri;
    let compositor = validate_compositor(&wm);
    let manager = WmConfigManager::new(wm);
    let config_changed = write_niri_instant_if_missing(&manager)?;
    let include_added = ensure_main_config_include(&manager, &wm)?;
    report_status(&wm, config_changed, include_added, &manager);
    if config_changed || include_added {
        maybe_reload_wm(&manager, &wm, &compositor);
    }
    Ok(())
}

/// Create an empty `instant.kdl` with a header if it does not exist yet.
///
/// Returns `true` when the file was created. Existing user content is never
/// overwritten — `ins settings` mutations operate on it incrementally.
fn write_niri_instant_if_missing(manager: &WmConfigManager) -> Result<bool> {
    if manager.config_path().exists() {
        return Ok(false);
    }
    manager.write_full_config(crate::common::compositor::config::INSTANT_KDL_HEADER)?;
    Ok(true)
}

fn ensure_main_config_exists(manager: &WmConfigManager) -> Result<()> {
    let main_config = manager.main_config_path();
    if main_config.exists() {
        return Ok(());
    }

    let output = instantwmctl::output(["config", "default"])?;

    if let Some(parent) = main_config.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    std::fs::write(main_config, &output.stdout)
        .with_context(|| format!("Failed to write {}", main_config.display()))?;

    emit(
        Level::Info,
        "setup.instantwm.config_created",
        &format!(
            "{} Created default config at {}",
            char::from(NerdFont::Check),
            main_config.display()
        ),
        None,
    );

    Ok(())
}

fn validate_compositor(wm: &WindowManager) -> CompositorType {
    let compositor = CompositorType::detect();

    if !compositor_matches_wm(wm, &compositor) {
        emit(
            Level::Warn,
            &format!("setup.{}.wrong_compositor", wm.name()),
            &format!(
                "{} Current compositor is {}, not {}. Setup will proceed but may not work correctly.",
                char::from(NerdFont::Warning),
                compositor.name(),
                wm.name()
            ),
            None,
        );
    }

    compositor
}

fn compositor_matches_wm(wm: &WindowManager, compositor: &CompositorType) -> bool {
    let expected_compositor = match wm {
        WindowManager::Sway => CompositorType::Sway,
        WindowManager::I3 => CompositorType::I3,
        WindowManager::InstantWM => CompositorType::InstantWM,
        WindowManager::Niri => CompositorType::Niri,
    };

    compositor == &expected_compositor
}

fn write_config_if_changed(manager: &WmConfigManager) -> Result<bool> {
    let expected_content = generate_wm_config(manager.wm())?;
    let disk_hash = manager.hash_config().unwrap_or(0);
    let expected_hash = hash_string(&expected_content);
    let changed = disk_hash != expected_hash;
    if changed {
        manager.write_full_config(&expected_content)?;
    }
    Ok(changed)
}

fn write_instantwm_config_if_changed(manager: &WmConfigManager) -> Result<bool> {
    let expected_content = crate::assist::mode_config::render_instantwm()?;
    let disk_hash = manager.hash_config().unwrap_or(0);
    let expected_hash = hash_string(&expected_content);
    let changed = disk_hash != expected_hash;
    if changed {
        manager.write_full_config(&expected_content)?;
    }
    Ok(changed)
}

fn ensure_main_config_include(manager: &WmConfigManager, wm: &WindowManager) -> Result<bool> {
    match manager.ensure_included_in_main_config() {
        Ok(added) => Ok(added),
        Err(e) => {
            emit(
                Level::Warn,
                &format!("setup.{}.include_failed", wm.name()),
                &format!(
                    "{} Could not add include to {} config: {}",
                    char::from(NerdFont::Warning),
                    wm.name(),
                    e
                ),
                None,
            );
            Ok(false)
        }
    }
}

fn report_status(
    wm: &WindowManager,
    config_changed: bool,
    include_added: bool,
    manager: &WmConfigManager,
) {
    if config_changed || include_added {
        emit(
            Level::Success,
            &format!("setup.{}.updated", wm.name()),
            &format!(
                "{} {} config updated",
                char::from(NerdFont::Check),
                wm.name()
            ),
            None,
        );
    } else {
        emit(
            Level::Info,
            &format!("setup.{}.unchanged", wm.name()),
            &format!(
                "{} {} config unchanged, skipping reload",
                char::from(NerdFont::Check),
                wm.name()
            ),
            None,
        );
    }
    emit(
        Level::Info,
        &format!("setup.{}.config_path", wm.name()),
        &format!("  Config file: {}", manager.config_path().display()),
        None,
    );
}

fn maybe_reload_wm(manager: &WmConfigManager, wm: &WindowManager, compositor: &CompositorType) {
    if !compositor_matches_wm(wm, compositor) {
        emit(
            Level::Warn,
            &format!("setup.{}.reload_skipped_wrong_compositor", wm.name()),
            &format!(
                "{} Skipping {} reload because current compositor is {}",
                char::from(NerdFont::Warning),
                wm.name(),
                compositor.name()
            ),
            None,
        );
        return;
    }

    match manager.reload() {
        Ok(()) => {
            emit(
                Level::Success,
                &format!("setup.{}.reloaded", wm.name()),
                &format!(
                    "{} {} configuration reloaded",
                    char::from(NerdFont::Sync),
                    wm.name()
                ),
                None,
            );
        }
        Err(e) => {
            emit(
                Level::Warn,
                &format!("setup.{}.reload_failed", wm.name()),
                &format!(
                    "{} Failed to reload {}: {}",
                    char::from(NerdFont::Warning),
                    wm.name(),
                    e
                ),
                None,
            );
        }
    }
}

/// Generate the full WM config content (sway/i3).
pub(crate) fn generate_wm_config(wm: WindowManager) -> Result<String> {
    use std::fmt::Write;

    let mut content = String::new();

    // Header
    writeln!(content, "# instantCLI {} configuration", wm.name())?;
    writeln!(
        content,
        "# This file is managed by instantCLI. Manual edits may be overwritten."
    )?;
    writeln!(content)?;

    // Cursor theme section (sway-only, i3 doesn't support this)
    if wm == WindowManager::Sway
        && let Ok(theme) = get_current_cursor_theme()
        && !theme.is_empty()
    {
        writeln!(content, "# --- BEGIN cursor_theme ---")?;
        writeln!(content, "seat * xcursor_theme {}", theme)?;
        writeln!(content, "# --- END cursor_theme ---")?;
        writeln!(content)?;
    }

    // Assist keybinds section
    writeln!(content, "# --- BEGIN assist ---")?;
    let keybinds = crate::assist::mode_config::render_sway_like(wm.name())?;
    write!(content, "{}", keybinds.trim())?;
    writeln!(content)?;
    writeln!(content, "# --- END assist ---")?;

    Ok(content)
}

/// Hash a string for comparison.
fn hash_string(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Get the current cursor theme from gsettings.
fn get_current_cursor_theme() -> Result<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "cursor-theme"])
        .output()
        .context("Failed to query cursor theme from gsettings")?;

    let theme = String::from_utf8_lossy(&output.stdout);
    // Remove quotes and whitespace
    Ok(theme
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositor_matches_target_window_manager() {
        assert!(compositor_matches_wm(
            &WindowManager::Sway,
            &CompositorType::Sway
        ));
        assert!(compositor_matches_wm(
            &WindowManager::I3,
            &CompositorType::I3
        ));
        assert!(compositor_matches_wm(
            &WindowManager::InstantWM,
            &CompositorType::InstantWM
        ));
        assert!(compositor_matches_wm(
            &WindowManager::Niri,
            &CompositorType::Niri
        ));
    }

    #[test]
    fn compositor_mismatch_prevents_target_reload() {
        assert!(!compositor_matches_wm(
            &WindowManager::I3,
            &CompositorType::Sway
        ));
        assert!(!compositor_matches_wm(
            &WindowManager::Sway,
            &CompositorType::I3
        ));
        assert!(!compositor_matches_wm(
            &WindowManager::InstantWM,
            &CompositorType::Sway
        ));
        assert!(!compositor_matches_wm(
            &WindowManager::Niri,
            &CompositorType::Sway
        ));
    }
}
