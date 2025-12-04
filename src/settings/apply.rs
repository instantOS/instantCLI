use anyhow::{Context, Result};

use crate::settings::registry::SETTINGS;

use super::context::{SettingsContext, apply_definition, make_apply_override};
use super::store::SettingsStore;

pub fn run_nonpersistent_apply(debug: bool, privileged_flag: bool) -> Result<()> {
    let store = SettingsStore::load().context("loading settings file")?;
    let mut ctx = SettingsContext::new(store, debug, privileged_flag);

    let mut applied = 0usize;

    for definition in SETTINGS
        .iter()
        .filter(|definition| definition.requires_reapply)
    {
        match definition.kind {
            crate::settings::registry::SettingKind::Toggle { .. }
            | crate::settings::registry::SettingKind::Choice { .. } => {
                ctx.emit_info(
                    "settings.apply.reapply",
                    &format!("Reapplying {}", definition.title),
                );
                apply_definition(&mut ctx, definition, None)?;
                applied += 1;
            }
            crate::settings::registry::SettingKind::Action { .. }
                if definition.id == "language.keyboard_layout" =>
            {
                ctx.emit_info(
                    "settings.apply.reapply",
                    &format!("Reapplying {}", definition.title),
                );
                crate::settings::actions::restore_keyboard_layout(&mut ctx)?;
                applied += 1;
            }
            crate::settings::registry::SettingKind::Action { .. }
                if definition.id == "desktop.mouse.sensitivity" =>
            {
                ctx.emit_info(
                    "settings.apply.reapply",
                    &format!("Reapplying {}", definition.title),
                );
                crate::settings::actions::restore_mouse_sensitivity(&mut ctx)?;
                applied += 1;
            }
            _ => {}
        }
    }

    if applied == 0 {
        ctx.emit_info(
            "settings.apply.none",
            "No non-persistent settings are currently enabled.",
        );
    } else {
        ctx.emit_success(
            "settings.apply.completed",
            &format!(
                "Reapplied {applied} setting{}",
                if applied == 1 { "" } else { "s" }
            ),
        );
    }

    Ok(())
}

pub fn run_internal_apply(
    debug: bool,
    privileged_flag: bool,
    setting_id: &str,
    bool_value: Option<bool>,
    string_value: Option<String>,
    settings_file: Option<std::path::PathBuf>,
) -> Result<()> {
    let store = if let Some(path) = settings_file {
        SettingsStore::load_from_path(path)
    } else {
        SettingsStore::load()
    }
    .context("loading settings file for privileged apply")?;

    let mut ctx = SettingsContext::new(store, debug, privileged_flag);

    let definition = SETTINGS
        .iter()
        .find(|definition| definition.id == setting_id)
        .with_context(|| format!("unknown setting id {setting_id}"))?;

    let override_value = make_apply_override(definition, bool_value, string_value);

    apply_definition(&mut ctx, definition, override_value)?;

    Ok(())
}
