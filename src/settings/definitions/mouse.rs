//! Mouse-related settings
//!
//! Natural scrolling, button swap, and mouse sensitivity settings.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::common::compositor::{CompositorType, sway};
use crate::menu::client::MenuClient;
use crate::menu::protocol::SliderRequest;
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{BoolSettingKey, IntSettingKey};
use crate::ui::prelude::*;

// ============================================================================
// Natural Scrolling
// ============================================================================

pub struct NaturalScroll;

impl NaturalScroll {
    const KEY: BoolSettingKey = BoolSettingKey::new("mouse.natural_scroll", false);
}

impl Setting for NaturalScroll {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("mouse.natural_scroll")
            .title("Natural Scrolling")
            .icon(NerdFont::Mouse)
            .summary("Reverse the scroll direction to match touchpad/touchscreen behavior.\n\nWhen enabled, scrolling up moves the content up (like pushing paper).\n\nSupports Sway and X11 window managers.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);
        self.apply_value(ctx, enabled)
    }

    fn apply_value(&self, ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
        apply_natural_scrolling(ctx, enabled)
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let enabled = ctx.bool(Self::KEY);
        if !enabled {
            return None;
        }
        Some(self.apply_value(ctx, enabled))
    }
}

// ============================================================================
// Swap Mouse Buttons
// ============================================================================

pub struct SwapButtons;

impl SwapButtons {
    const KEY: BoolSettingKey = BoolSettingKey::new("mouse.swap_buttons", false);
}

impl Setting for SwapButtons {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("mouse.swap_buttons")
            .title("Swap Mouse Buttons")
            .icon(NerdFont::Mouse)
            .summary("Swap left and right mouse buttons for left-handed use.\n\nWhen enabled, the right button becomes the primary click.\n\nCurrently only supported on X11.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);
        self.apply_value(ctx, enabled)
    }

    fn apply_value(&self, ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
        apply_swap_buttons(ctx, enabled)
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let enabled = ctx.bool(Self::KEY);
        if !enabled {
            return None;
        }
        Some(self.apply_value(ctx, enabled))
    }
}

// ============================================================================
// Mouse Sensitivity
// ============================================================================

pub struct MouseSensitivity;

impl MouseSensitivity {
    const KEY: IntSettingKey = IntSettingKey::new("desktop.mouse.sensitivity", 50);
}

impl Setting for MouseSensitivity {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("mouse.sensitivity")
            .title("Mouse Sensitivity")
            .icon(NerdFont::Mouse)
            .summary("Adjust mouse pointer speed using an interactive slider.\n\nThe setting will be automatically restored on login.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::MouseSensitivity))
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let initial_value = if ctx.contains(Self::KEY.key) {
            Some(ctx.int(Self::KEY))
        } else {
            None
        };

        if let Some(value) = run_mouse_speed_slider(initial_value)? {
            ctx.set_int(Self::KEY, value);
            ctx.notify(
                "Mouse Sensitivity",
                &format!("Mouse sensitivity set to {}", value),
            );
        }
        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        if !ctx.contains(Self::KEY.key) {
            return None;
        }

        let value = ctx.int(Self::KEY);
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
        Some(Ok(()))
    }
}

fn run_mouse_speed_slider(initial_value: Option<i64>) -> Result<Option<i64>> {
    let start_value = initial_value.unwrap_or(50);

    let client = MenuClient::new();
    client.ensure_server_running()?;

    let current_exe = std::env::current_exe()?;
    let program = current_exe.to_string_lossy().to_string();
    let args = vec![program, "assist".to_string(), "mouse-speed-set".to_string()];

    let request = SliderRequest {
        min: 0,
        max: 100,
        value: Some(start_value),
        step: Some(1),
        big_step: Some(10),
        label: Some("Mouse Sensitivity".to_string()),
        command: args,
    };

    client.slide(request)
}

// ============================================================================
// Shared Helpers
// ============================================================================

/// Apply natural scrolling setting (shared by both apply and restore)
pub fn apply_natural_scrolling(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();
    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_x11 = compositor.is_x11();

    if !is_sway && !is_x11 {
        ctx.emit_unsupported(
            "settings.mouse.natural_scroll.unsupported",
            &format!(
                "Natural scrolling configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    if is_sway {
        let value = if enabled { "enabled" } else { "disabled" };
        let pointer_cmd = format!("input type:pointer natural_scroll {}", value);
        let touchpad_cmd = format!("input type:touchpad natural_scroll {}", value);

        let pointer_result = sway::swaymsg(&pointer_cmd);
        let touchpad_result = sway::swaymsg(&touchpad_cmd);

        if let (Err(e1), Err(e2)) = (&pointer_result, &touchpad_result) {
            ctx.emit_info(
                "settings.mouse.natural_scroll.sway_failed",
                &format!(
                    "Failed to apply natural scrolling in Sway: pointer: {e1}, touchpad: {e2}"
                ),
            );
            return Ok(());
        }

        ctx.notify(
            "Natural Scrolling",
            if enabled {
                "Natural scrolling enabled"
            } else {
                "Natural scrolling disabled"
            },
        );
    } else {
        let value = if enabled { "1" } else { "0" };
        let applied = apply_libinput_property_helper(
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
    }

    Ok(())
}

/// Apply swap buttons setting (shared by both apply and restore)
pub fn apply_swap_buttons(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();

    if !compositor.is_x11() {
        ctx.emit_unsupported(
            "settings.mouse.swap_buttons.unsupported",
            &format!(
                "Swap mouse buttons configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    let value = if enabled { "1" } else { "0" };
    let applied = apply_libinput_property_helper(
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

/// Get all pointer device IDs from xinput
pub fn get_pointer_device_ids() -> Result<Vec<String>> {
    let output = Command::new("xinput")
        .arg("list")
        .arg("--id-only")
        .output()
        .context("Failed to run xinput list")?;

    if !output.status.success() {
        bail!("xinput list failed");
    }

    let all_ids: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.trim().to_string())
        .collect();

    let mut pointer_ids = Vec::new();
    for id in all_ids {
        if let Ok(props_output) = Command::new("xinput").arg("list-props").arg(&id).output() {
            let props = String::from_utf8_lossy(&props_output.stdout);
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
pub fn apply_libinput_property_helper(
    property_name: &str,
    value: &str,
    error_key: &str,
) -> Result<usize> {
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
