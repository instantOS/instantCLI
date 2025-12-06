//! Setting state computation
//!
//! Computes the display state for a setting based on its current value.

use crate::settings::setting::{Setting, SettingType};

use super::super::context::SettingsContext;
use super::items::SettingState;

/// Compute the display state for a setting
pub fn compute_setting_state(ctx: &SettingsContext, setting: &'static dyn Setting) -> SettingState {
    match setting.setting_type() {
        SettingType::Toggle { key } => SettingState::Toggle {
            enabled: ctx.bool(key),
        },
        SettingType::Choice { key } => {
            let current = ctx.string(key);
            SettingState::Choice {
                current_label: if current.is_empty() {
                    "Not set"
                } else {
                    Box::leak(current.into_boxed_str())
                },
            }
        }
        SettingType::Action => SettingState::Action,
        SettingType::Command => SettingState::Command,
    }
}
