//! Brightness-related settings actions
//!
//! Handles screen brightness configuration with persistence and restore on login.

use anyhow::{Context, Result};
use std::process::Command;

use crate::menu::client::MenuClient;
use crate::menu::protocol::SliderRequest;
use crate::ui::prelude::*;

use super::super::context::SettingsContext;

/// Set screen brightness using brightnessctl
pub fn set_brightness(value: i64) -> Result<()> {
    let output = Command::new("brightnessctl")
        .args(["--quiet", "set", &format!("{}%", value)])
        .output()
        .context("Failed to run brightnessctl")?;

    if !output.status.success() {
        anyhow::bail!(
            "brightnessctl failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Get current brightness percentage from brightnessctl
fn get_current_brightness() -> Option<i64> {
    let current = Command::new("brightnessctl")
        .arg("get")
        .output()
        .ok()?
        .stdout;
    let current: f64 = String::from_utf8_lossy(&current).trim().parse().ok()?;

    let max = Command::new("brightnessctl")
        .arg("max")
        .output()
        .ok()?
        .stdout;
    let max: f64 = String::from_utf8_lossy(&max).trim().parse().ok()?;

    if max <= 0.0 {
        return None;
    }

    let percent = (current / max * 100.0).round().clamp(0.0, 100.0);
    Some(percent as i64)
}

/// Run the brightness slider and return the selected value
fn run_brightness_slider(initial_value: Option<i64>) -> Result<Option<i64>> {
    let start_value = initial_value.or_else(get_current_brightness).unwrap_or(50);

    let client = MenuClient::new();
    client.ensure_server_running()?;

    // Use the current executable to set brightness on slider changes
    let current_exe = std::env::current_exe()?;
    let program = current_exe.to_string_lossy().to_string();
    let args = vec![program, "assist".to_string(), "brightness-set".to_string()];

    let request = SliderRequest {
        min: 0,
        max: 100,
        value: Some(start_value),
        step: Some(1),
        big_step: Some(10),
        label: Some("Screen Brightness".to_string()),
        command: args,
    };

    client.slide(request)
}

/// Interactive brightness configuration through the settings UI
pub fn configure_brightness(ctx: &mut SettingsContext) -> Result<()> {
    let key = super::super::store::IntSettingKey::new("appearance.brightness", 50);

    // Try to get stored value first, to initialize slider
    let initial_value = if ctx.contains(key.key) {
        Some(ctx.int(key))
    } else {
        None
    };

    match run_brightness_slider(initial_value)? {
        Some(value) => {
            ctx.set_int(key, value);
            ctx.notify(
                "Screen Brightness",
                &format!("Screen brightness set to {}%", value),
            );
        }
        None => {
            // Cancelled
        }
    }
    Ok(())
}

/// Restore brightness setting on login/apply
pub fn restore_brightness(ctx: &mut SettingsContext) -> Result<()> {
    let key = super::super::store::IntSettingKey::new("appearance.brightness", 50);

    // Only restore if explicitly set in config
    if ctx.contains(key.key) {
        let value = ctx.int(key);
        if let Err(e) = set_brightness(value) {
            emit(
                Level::Warn,
                "settings.brightness.restore_failed",
                &format!("Failed to restore brightness: {e}"),
                None,
            );
        } else {
            emit(
                Level::Debug,
                "settings.brightness.restored",
                &format!("Restored brightness: {}%", value),
                None,
            );
        }
    }
    Ok(())
}
