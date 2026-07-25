//! Clipboard settings
//!
//! Clipboard history and management tools.

use anyhow::Result;

use crate::common::display_server::DisplayServer;
use crate::common::package::InstallResult;
use crate::common::systemd::SystemdManager;
use crate::settings::context::SettingsContext;
use crate::settings::deps::{CLIPHIST, CLIPMENU};
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
            .build()
    }

    fn setting_type(&self) -> SettingType {
        // We don't store state in TOML anymore, we derive it from systemd
        SettingType::Action
    }

    fn get_display_state(&self, _ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        use crate::settings::setting::SettingState;

        let (dependency, service) = clipboard_backend();
        if !dependency.is_installed() {
            return SettingState::Toggle { enabled: false };
        }

        let systemd = SystemdManager::user();
        let enabled = systemd.is_enabled(service) || systemd.is_active(service);

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

        let (dependency, service) = clipboard_backend();

        if should_enable {
            match dependency.ensure()? {
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
            let other_service = if service == "cliphist.service" {
                "clipmenud.service"
            } else {
                "cliphist.service"
            };
            if systemd.is_enabled(other_service) || systemd.is_active(other_service) {
                systemd.disable_and_stop(other_service)?;
            }
            if !systemd.is_enabled(service) {
                systemd.enable_and_start(service)?;
            } else if !systemd.is_active(service) {
                systemd.start(service)?;
            }

            ctx.notify("Clipboard manager", "Clipboard history enabled");
        } else {
            // Disable
            let systemd = SystemdManager::user();
            if systemd.is_enabled(service) || systemd.is_active(service) {
                systemd.disable_and_stop(service)?;
                ctx.notify("Clipboard manager", "Clipboard history disabled");
            }
        }

        Ok(())
    }
}

fn clipboard_backend() -> (&'static crate::common::package::Dependency, &'static str) {
    if DisplayServer::detect().is_wayland() {
        (&CLIPHIST, "cliphist.service")
    } else {
        (&CLIPMENU, "clipmenud.service")
    }
}
