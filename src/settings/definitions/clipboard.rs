//! Clipboard settings
//!
//! Clipboard history and management tools.

use anyhow::Result;

use crate::common::package::InstallResult;
use crate::common::systemd::SystemdManager;
use crate::settings::context::SettingsContext;
use crate::settings::deps::CLIPMENU;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// Clipboard Manager
// ============================================================================

pub struct ClipboardManager;

impl Setting for ClipboardManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("desktop.clipboard")
            .title("Clipboard History")
            .icon(NerdFont::Clipboard)
            .summary("Remember your copy/paste history so you can access previously copied items.\n\nWhen enabled, you can paste from your clipboard history instead of just the last copied item.")
            .requirements(vec![&CLIPMENU])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        // We don't store state in TOML anymore, we derive it from systemd
        SettingType::Action
    }

    fn get_display_state(&self, _ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        use crate::settings::setting::SettingState;

        // Check if package is installed first
        if !CLIPMENU.is_installed() {
            return SettingState::Toggle { enabled: false };
        }

        // Check systemd service status
        let systemd = SystemdManager::user();
        let enabled = systemd.is_enabled("clipmenud") || systemd.is_active("clipmenud");

        SettingState::Toggle { enabled }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        use crate::settings::setting::SettingState;

        let current_state = self.get_display_state(ctx);
        let currently_enabled = match current_state {
            SettingState::Toggle { enabled } => enabled,
            _ => false,
        };

        // Toggle logic
        let should_enable = !currently_enabled;

        const CLIPMENU_SERVICE: &str = "clipmenud";

        if should_enable {
            // Ensure package is installed before trying to enable service
            match CLIPMENU.ensure()? {
                InstallResult::Installed | InstallResult::AlreadyInstalled => {}
                _ => {
                    ctx.emit_info(
                        "settings.clipboard.aborted",
                        "Clipboard history setup was cancelled.",
                    );
                    return Ok(());
                }
            }

            let systemd = SystemdManager::user();
            if !systemd.is_enabled(CLIPMENU_SERVICE) {
                systemd.enable_and_start(CLIPMENU_SERVICE)?;
            } else if !systemd.is_active(CLIPMENU_SERVICE) {
                systemd.start(CLIPMENU_SERVICE)?;
            }

            ctx.notify("Clipboard manager", "Clipboard history enabled");
        } else {
            // Disable
            let systemd = SystemdManager::user();
            if systemd.is_enabled(CLIPMENU_SERVICE) || systemd.is_active(CLIPMENU_SERVICE) {
                systemd.disable_and_stop(CLIPMENU_SERVICE)?;
                ctx.notify("Clipboard manager", "Clipboard history disabled");
            }
        }

        Ok(())
    }
}
