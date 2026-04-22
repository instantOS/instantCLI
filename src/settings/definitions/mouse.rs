//! Mouse-related settings
//!
//! Natural scrolling, button swap, and mouse sensitivity settings.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::assist::{AssistInternalCommand, assist_command_argv};
use crate::common::compositor::{CompositorType, niri, sway};
use crate::common::instantwmctl;
use crate::menu::client::MenuClient;
use crate::menu::protocol::SliderRequest;
use crate::menu_utils::{FzfPreview, FzfSelectable, MenuCursor, select_one_with_style_at};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{BoolSettingKey, IntSettingKey, StringSettingKey};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::prelude::*;

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
            .summary("Reverse the scroll direction to match touchpad/touchscreen behavior.\n\nWhen enabled, scrolling up moves the content up (like pushing paper).\n\nSupports Sway, InstantWM, and X11 window managers.")
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
            .summary("Swap left and right mouse buttons for left-handed use.\n\nWhen enabled, the right button becomes the primary click.\n\nSupports InstantWM and X11.")
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
// Tap-to-Click
// ============================================================================

pub struct TapToClick;

impl TapToClick {
    const KEY: BoolSettingKey = BoolSettingKey::new("mouse.tap", false);
}

impl Setting for TapToClick {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("mouse.tap")
            .title("Tap-to-Click")
            .icon(NerdFont::Mouse)
            .summary("Enable or disable tap-to-click on touchpads.\n\nWhen enabled, tapping on the touchpad surface acts as a mouse click.\n\nSupports InstantWM.")
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
        apply_tap_to_click(ctx, enabled)
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
// Acceleration Profile
// ============================================================================

pub struct AccelProfile;

impl AccelProfile {
    const KEY: StringSettingKey = StringSettingKey::new("mouse.accel_profile", "adaptive");
}

const ACCEL_PROFILE_OPTIONS: &[AccelProfileChoice] = &[
    AccelProfileChoice {
        value: "flat",
        label: "Flat",
        description: "Disables pointer acceleration. Move the cursor at a constant speed regardless of movement velocity.",
    },
    AccelProfileChoice {
        value: "adaptive",
        label: "Adaptive",
        description: "Applies dynamic acceleration based on movement velocity. Faster movements result in greater cursor displacement.",
    },
];

struct AccelProfileChoice {
    value: &'static str,
    label: &'static str,
    description: &'static str,
}

impl Setting for AccelProfile {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("mouse.accel_profile")
            .title("Acceleration Profile")
            .icon(NerdFont::Mouse)
            .summary("Choose how pointer acceleration behaves.\n\n\"Flat\" provides constant cursor speed regardless of movement speed.\n\"Adaptive\" applies dynamic acceleration - faster movements result in greater cursor travel.\n\nSupports niri and InstantWM.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Choice { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.string(Self::KEY);
        let initial_index = ACCEL_PROFILE_OPTIONS
            .iter()
            .position(|o| o.value == current)
            .unwrap_or(1);

        let mut cursor = MenuCursor::new();

        loop {
            let items = build_accel_profile_items(&ctx.string(Self::KEY));
            let initial_cursor = cursor.initial_index(&items).or(Some(initial_index));
            let selection = select_one_with_style_at(items.clone(), initial_cursor)?;

            match selection {
                Some(display) => {
                    cursor.update(&display, &items);

                    match display.choice {
                        Some(choice) => {
                            ctx.set_string(Self::KEY, choice.value);
                            apply_accel_profile(ctx, choice.value)?;
                        }
                        None => break,
                    }
                }
                None => break,
            }
        }

        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::InstantWM | CompositorType::Niri) {
            return None;
        }

        let profile = ctx.string(Self::KEY);
        Some(apply_accel_profile(ctx, &profile))
    }
}

// ============================================================================
// Scroll Factor
// ============================================================================

pub struct ScrollFactor;

impl ScrollFactor {
    const KEY: IntSettingKey = IntSettingKey::new("mouse.scroll_factor", 100);
}

impl Setting for ScrollFactor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("mouse.scroll_factor")
            .title("Scroll Speed")
            .icon(NerdFont::Mouse)
            .summary("Adjust the scroll wheel speed multiplier.\n\nValues above 100% increase scroll speed, values below 100% decrease it.\n\nSupports InstantWM.")
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

        let start_value = initial_value.unwrap_or(100);

        let client = MenuClient::new();
        client.ensure_server_running()?;

        let args = assist_command_argv(AssistInternalCommand::ScrollFactorSet)?;
        let request = SliderRequest {
            min: 0,
            max: 300,
            value: Some(start_value),
            step: Some(5),
            big_step: Some(25),
            label: Some("Scroll Speed".to_string()),
            command: args,
        };

        if let Some(value) = client.slide(request)? {
            ctx.set_int(Self::KEY, value);
            ctx.notify("Scroll Speed", &format!("Scroll factor set to {}%", value));
        }
        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        if !ctx.contains(Self::KEY.key) {
            return None;
        }

        let value = ctx.int(Self::KEY);
        if let Err(e) = apply_scroll_factor(ctx, value) {
            emit(
                Level::Warn,
                "settings.mouse.scroll_factor.restore_failed",
                &format!("Failed to restore scroll factor: {e}"),
                None,
            );
        } else {
            emit(
                Level::Debug,
                "settings.mouse.scroll_factor.restored",
                &format!("Restored scroll factor: {}%", value),
                None,
            );
        }
        Some(Ok(()))
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
            .summary("Adjust mouse pointer speed using an interactive slider.\n\nThe setting will be automatically restored on login.\n\nSupports niri, Sway, InstantWM, GNOME, and X11.")
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

        if let Some(value) = crate::assist::actions::mouse::run_mouse_speed_slider(initial_value)? {
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

// ============================================================================
// Shared Helpers
// ============================================================================

/// Apply natural scrolling setting (shared by both apply and restore)
pub fn apply_natural_scrolling(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();
    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_instantwm = matches!(compositor, CompositorType::InstantWM);
    let is_x11 = compositor.is_x11();

    if !is_sway && !is_x11 && !is_instantwm {
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
    } else if is_instantwm {
        let value = if enabled { "enabled" } else { "disabled" };

        let pointer_result = instantwmctl::run([
            "mouse",
            "natural-scroll",
            value,
            "--identifier",
            "type:pointer",
        ]);
        let touchpad_result = instantwmctl::run([
            "mouse",
            "natural-scroll",
            value,
            "--identifier",
            "type:touchpad",
        ]);

        if let (Err(e1), Err(e2)) = (&pointer_result, &touchpad_result) {
            ctx.emit_info(
                "settings.mouse.natural_scroll.instantwm_failed",
                &format!(
                    "Failed to apply natural scrolling in instantWM: pointer: {e1}, touchpad: {e2}"
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
    let is_instantwm = matches!(compositor, CompositorType::InstantWM);
    let is_x11 = compositor.is_x11();

    if !is_x11 && !is_instantwm {
        ctx.emit_unsupported(
            "settings.mouse.swap_buttons.unsupported",
            &format!(
                "Swap mouse buttons configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    if is_instantwm {
        let value = if enabled { "enabled" } else { "disabled" };

        let pointer_result = std::process::Command::new("instantwmctl")
            .args(["mouse", "left-handed", "type:pointer", value])
            .status();
        let touchpad_result = std::process::Command::new("instantwmctl")
            .args(["mouse", "left-handed", "type:touchpad", value])
            .status();

        if let (Err(e1), Err(e2)) = (&pointer_result, &touchpad_result) {
            ctx.emit_info(
                "settings.mouse.swap_buttons.instantwm_failed",
                &format!(
                    "Failed to apply swap mouse buttons in instantWM: pointer: {e1}, touchpad: {e2}"
                ),
            );
            return Ok(());
        }

        ctx.notify(
            "Swap Mouse Buttons",
            if enabled {
                "Mouse buttons swapped (left-handed mode)"
            } else {
                "Mouse buttons normal (right-handed mode)"
            },
        );
    } else {
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

/// Build the display items list with current selection marked
fn build_accel_profile_items(current: &str) -> Vec<AccelProfileChoiceDisplay> {
    let mut items: Vec<AccelProfileChoiceDisplay> = ACCEL_PROFILE_OPTIONS
        .iter()
        .map(|choice| AccelProfileChoiceDisplay {
            choice: Some(choice),
            is_current: choice.value == current,
        })
        .collect();

    items.push(AccelProfileChoiceDisplay {
        choice: None,
        is_current: false,
    });

    items
}

#[derive(Clone)]
struct AccelProfileChoiceDisplay {
    choice: Option<&'static AccelProfileChoice>,
    is_current: bool,
}

impl FzfSelectable for AccelProfileChoiceDisplay {
    fn fzf_display_text(&self) -> String {
        match self.choice {
            Some(choice) => {
                let icon = if self.is_current {
                    format_icon_colored(NerdFont::CheckSquare, colors::GREEN)
                } else {
                    format_icon_colored(NerdFont::Square, colors::OVERLAY1)
                };
                format!("{} {}", icon, choice.label)
            }
            None => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self.choice {
            Some(choice) => FzfPreview::Text(choice.description.to_string()),
            None => FzfPreview::Text("Go back to the previous menu".to_string()),
        }
    }

    fn fzf_key(&self) -> String {
        match self.choice {
            Some(choice) => choice.value.to_string(),
            None => "__back__".to_string(),
        }
    }
}

/// Apply tap-to-click setting
fn apply_tap_to_click(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let compositor = CompositorType::detect();

    if !matches!(compositor, CompositorType::InstantWM) {
        ctx.emit_unsupported(
            "settings.mouse.tap.unsupported",
            &format!(
                "Tap-to-click configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    let value = if enabled { "enabled" } else { "disabled" };

    let pointer_result = instantwmctl::run(["mouse", "tap", value]);

    if let Err(e) = &pointer_result {
        ctx.emit_info(
            "settings.mouse.tap.instantwm_failed",
            &format!("Failed to apply tap-to-click in instantWM: {e}"),
        );
        return Ok(());
    }

    ctx.notify(
        "Tap-to-Click",
        if enabled {
            "Tap-to-click enabled"
        } else {
            "Tap-to-click disabled"
        },
    );

    Ok(())
}

/// Apply acceleration profile setting
fn apply_accel_profile(ctx: &mut SettingsContext, profile: &str) -> Result<()> {
    let compositor = CompositorType::detect();

    if !matches!(compositor, CompositorType::InstantWM | CompositorType::Niri) {
        ctx.emit_unsupported(
            "settings.mouse.accel_profile.unsupported",
            &format!(
                "Acceleration profile configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    match compositor {
        CompositorType::InstantWM => {
            let result = instantwmctl::run(["mouse", "accel-profile", profile]);

            if let Err(e) = &result {
                ctx.emit_info(
                    "settings.mouse.accel_profile.instantwm_failed",
                    &format!("Failed to apply acceleration profile in instantWM: {e}"),
                );
                return Ok(());
            }
        }
        CompositorType::Niri => {
            if let Err(e) = niri::set_mouse_accel_profile(profile) {
                ctx.emit_info(
                    "settings.mouse.accel_profile.niri_failed",
                    &format!("Failed to apply acceleration profile in niri: {e}"),
                );
                return Ok(());
            }
        }
        _ => {}
    }

    let profile_label = if profile == "flat" {
        "Flat"
    } else {
        "Adaptive"
    };
    ctx.notify("Acceleration Profile", &format!("Set to {}", profile_label));

    Ok(())
}

/// Apply scroll factor setting
pub fn apply_scroll_factor(ctx: &mut SettingsContext, value: i64) -> Result<()> {
    let compositor = CompositorType::detect();

    if !matches!(compositor, CompositorType::InstantWM) {
        ctx.emit_unsupported(
            "settings.mouse.scroll_factor.unsupported",
            &format!(
                "Scroll factor configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    let factor = value as f64 / 100.0;

    let factor_arg = factor.to_string();
    let result = instantwmctl::run(["mouse", "scroll-factor", factor_arg.as_str()]);

    if let Err(e) = &result {
        ctx.emit_info(
            "settings.mouse.scroll_factor.instantwm_failed",
            &format!("Failed to apply scroll factor in instantWM: {e}"),
        );
        return Ok(());
    }

    ctx.notify("Scroll Speed", &format!("Scroll factor set to {}%", value));

    Ok(())
}
