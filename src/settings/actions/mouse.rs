//! Mouse-related settings actions
//!
//! Handles mouse sensitivity, natural scrolling, and button swap settings.

use anyhow::Result;

use crate::common::compositor::{CompositorType, sway};
use crate::ui::prelude::*;

use super::super::context::SettingsContext;
use super::super::definitions::mouse::{apply_libinput_property_helper, apply_natural_scrolling, apply_swap_buttons};

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

/// Apply natural scrolling setting (Sway and X11)
pub fn apply_natural_scroll(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    apply_natural_scrolling(ctx, enabled)
}

/// Apply swap mouse buttons setting (X11 only for now)
pub fn apply_swap_buttons(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    super::super::definitions::mouse::apply_swap_buttons(ctx, enabled)
}
