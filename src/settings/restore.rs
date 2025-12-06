//! Restore loop for settings that require reapplication after login/reboot

use anyhow::Result;

use super::context::SettingsContext;
use super::setting::settings_requiring_reapply;
use crate::ui::prelude::*;

/// Restore all settings that need reapplication on login
pub fn restore_settings(ctx: &mut SettingsContext) -> Result<usize> {
    let mut applied = 0usize;

    for setting in settings_requiring_reapply() {
        let metadata = setting.metadata();

        if let Some(result) = setting.restore(ctx) {
            ctx.emit_info(
                "settings.apply.reapply",
                &format!("Reapplying {}", metadata.title),
            );

            if let Err(e) = result {
                emit(
                    Level::Warn,
                    "settings.apply.failed",
                    &format!("Failed to reapply {}: {e}", metadata.title),
                    None,
                );
            } else {
                applied += 1;
            }
        }
    }

    Ok(applied)
}
