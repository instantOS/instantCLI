//! Screen brightness setting

use anyhow::{Context, Result};
use std::process::Command;

use crate::assist::{assist_command_argv, AssistInternalCommand};
use crate::menu::client::MenuClient;
use crate::menu::protocol::SliderRequest;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::IntSettingKey;
use crate::ui::prelude::*;

/// Screen brightness control
pub struct Brightness;

impl Brightness {
    const KEY: IntSettingKey = IntSettingKey::new("appearance.brightness", 50);
}

impl Setting for Brightness {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.brightness")
            .title("Screen Brightness")
            .icon(NerdFont::Lightbulb)
            .summary("Adjust screen brightness using an interactive slider.\n\nThe setting will be automatically restored on login.\n\nTip: You can also access this via instantASSIST (Super+A b).")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let initial_value = if ctx.contains(Self::KEY.key) {
            Some(ctx.int(Self::KEY))
        } else {
            None
        };

        match run_brightness_slider(initial_value)? {
            Some(value) => {
                ctx.set_int(Self::KEY, value);
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

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        if !ctx.contains(Self::KEY.key) {
            return None;
        }

        let value = ctx.int(Self::KEY);
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
        Some(Ok(()))
    }
}

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

    let args = assist_command_argv(AssistInternalCommand::BrightnessSet)?;

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
