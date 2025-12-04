//! Mouse-related settings actions
//!
//! Handles mouse sensitivity, natural scrolling, and button swap settings.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::common::compositor::CompositorType;
use crate::ui::prelude::*;

use super::super::context::SettingsContext;

pub fn configure_mouse_sensitivity(ctx: &mut SettingsContext) -> Result<()> {
    let key = super::super::store::IntSettingKey::new("desktop.mouse.sensitivity", 50);

    // Try to get stored value first, to initialize slider
    let initial_value = if ctx.contains(key.key) {
        Some(ctx.int(key))
    } else {
        None
    };

    match crate::assist::actions::mouse::run_mouse_speed_slider(initial_value)? {
        Some(value) => {
            ctx.set_int(key, value);
            ctx.notify(
                "Mouse Sensitivity",
                &format!("Mouse sensitivity set to {}", value),
            );
        }
        None => {
            // Cancelled
        }
    }
    Ok(())
}

pub fn restore_mouse_sensitivity(ctx: &mut SettingsContext) -> Result<()> {
    let key = super::super::store::IntSettingKey::new("desktop.mouse.sensitivity", 50);

    // Only restore if explicitly set in config
    if ctx.contains(key.key) {
        let value = ctx.int(key);
        // We use set_mouse_speed from assist module
        if let Err(e) = crate::assist::actions::mouse::set_mouse_speed(value) {
            emit(
                Level::Warn,
                "settings.mouse.restore_failed",
                &format!("Failed to restore mouse sensitivity: {e}"),
                None,
            );
        } else {
            emit(
                Level::Debug,
                "settings.mouse.restored",
                &format!("Restored mouse sensitivity: {value}"),
                None,
            );
        }
    }
    Ok(())
}

/// Get all pointer device IDs from xinput
fn get_pointer_device_ids() -> Result<Vec<String>> {
    let output = Command::new("xinput")
        .arg("list")
        .arg("--id-only")
        .output()
        .context("Failed to run xinput list")?;

    if !output.status.success() {
        bail!("xinput list failed");
    }

    // Get all device IDs
    let all_ids: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.trim().to_string())
        .collect();

    // Filter to only pointer devices by checking their properties
    let mut pointer_ids = Vec::new();
    for id in all_ids {
        // Check if this device has pointer-related properties
        if let Ok(props_output) = Command::new("xinput").arg("list-props").arg(&id).output() {
            let props = String::from_utf8_lossy(&props_output.stdout);
            // Device is a pointer if it has button map or natural scrolling property
            if props.contains("libinput Natural Scrolling Enabled")
                || props.contains("Button Labels")
            {
                pointer_ids.push(id);
            }
        }
    }

    Ok(pointer_ids)
}

/// Apply a libinput property to all pointer devices that support it
fn apply_libinput_property(property_name: &str, value: &str, error_key: &str) -> Result<usize> {
    let device_ids = get_pointer_device_ids()?;
    let mut applied = 0;

    for id in device_ids {
        if let Ok(props_output) = Command::new("xinput").arg("list-props").arg(&id).output() {
            let props = String::from_utf8_lossy(&props_output.stdout);
            if props.contains(property_name) {
                if let Err(e) = Command::new("xinput")
                    .args(["--set-prop", &id, property_name, value])
                    .status()
                {
                    emit(
                        Level::Warn,
                        error_key,
                        &format!("Failed to set {property_name} for device {id}: {e}"),
                        None,
                    );
                } else {
                    applied += 1;
                }
            }
        }
    }

    Ok(applied)
}

/// Apply natural scrolling setting (X11 only for now)
pub fn apply_natural_scroll(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();

    if !compositor.is_x11() {
        ctx.emit_info(
            "settings.mouse.natural_scroll.unsupported",
            &format!(
                "Natural scrolling configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    let value = if enabled { "1" } else { "0" };
    let applied = apply_libinput_property(
        "libinput Natural Scrolling Enabled",
        value,
        "settings.mouse.natural_scroll.device_failed",
    )?;

    if applied > 0 {
        ctx.notify(
            "Natural Scrolling",
            if enabled {
                "Natural scrolling enabled"
            } else {
                "Natural scrolling disabled"
            },
        );
    } else {
        ctx.emit_info(
            "settings.mouse.natural_scroll.no_devices",
            "No devices found that support natural scrolling.",
        );
    }

    Ok(())
}

/// Apply swap mouse buttons setting (X11 only for now)
pub fn apply_swap_buttons(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();

    if !compositor.is_x11() {
        ctx.emit_info(
            "settings.mouse.swap_buttons.unsupported",
            &format!(
                "Swap mouse buttons configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    let value = if enabled { "1" } else { "0" };
    let applied = apply_libinput_property(
        "libinput Left Handed Enabled",
        value,
        "settings.mouse.swap_buttons.device_failed",
    )?;

    if applied > 0 {
        ctx.notify(
            "Swap Mouse Buttons",
            if enabled {
                "Mouse buttons swapped (left-handed mode)"
            } else {
                "Mouse buttons normal (right-handed mode)"
            },
        );
    } else {
        ctx.emit_info(
            "settings.mouse.swap_buttons.no_devices",
            "No devices found that support button swapping.",
        );
    }

    Ok(())
}
