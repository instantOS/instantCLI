//! Animations setting
//!
//! Control desktop animations and visual effects (instantwm only).

use anyhow::Result;

use crate::common::compositor::CompositorType;
use crate::common::instantwm::InstantWmController;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::BoolSettingKey;
use crate::ui::prelude::*;

pub struct Animations;

impl Animations {
    const KEY: BoolSettingKey = BoolSettingKey::new("appearance.animations", true);
}

impl Setting for Animations {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.animations")
            .title("Animations")
            .icon(NerdFont::Magic)
            .summary("Enable smooth animations and visual effects on the desktop.\n\nDisable for better performance on older hardware.\n\nOnly supported on instantwm.")
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
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::InstantWM) {
            ctx.emit_unsupported(
                "settings.appearance.animations.unsupported",
                &format!(
                    "Animation configuration is only supported on instantwm. Detected: {}. Setting saved but not applied.",
                    compositor.name()
                ),
            );
            return Ok(());
        }

        let controller = InstantWmController::new();
        match controller.set_animations(enabled) {
            Ok(()) => {
                ctx.notify("Animations", if enabled { "Enabled" } else { "Disabled" });
            }
            Err(err) => {
                ctx.emit_failure(
                    "settings.appearance.animations.error",
                    &format!("Failed to apply animation setting: {err}"),
                );
            }
        }

        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::InstantWM) {
            return None;
        }
        Some(self.apply_value(ctx, ctx.bool(Self::KEY)))
    }
}
