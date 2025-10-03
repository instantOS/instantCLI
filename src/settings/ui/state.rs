use super::super::context::SettingsContext;
use super::super::registry::{SettingDefinition, SettingKind};
use super::items::SettingState;

pub fn compute_setting_state(
    ctx: &SettingsContext,
    definition: &'static SettingDefinition,
) -> SettingState {
    match &definition.kind {
        SettingKind::Toggle { key, .. } => SettingState::Toggle {
            enabled: ctx.bool(*key),
        },
        SettingKind::Choice { key, options, .. } => {
            let current_value = ctx.string(*key);
            let current_index = options
                .iter()
                .position(|option| option.value == current_value);
            SettingState::Choice { current_index }
        }
        SettingKind::Action { .. } => SettingState::Action,
        SettingKind::Command { .. } => SettingState::Command,
    }
}

pub fn format_setting_path(
    category: &super::super::registry::SettingCategory,
    definition: &SettingDefinition,
) -> String {
    let mut segments = Vec::with_capacity(1 + definition.breadcrumbs.len());
    segments.push(category.title);
    segments.extend(definition.breadcrumbs.iter().copied());
    segments.join(" -> ")
}
