//! Clipboard settings
//!
//! Clipboard history and management tools.

use anyhow::Result;

use crate::clip::{ClipBackend, capture_status, disable_capture, enable_capture};
use crate::settings::context::SettingsContext;
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
        // State is derived from systemd, not stored in TOML.
        SettingType::Action
    }

    fn get_display_state(&self, _ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        use crate::settings::setting::SettingState;

        let enabled = ClipBackend::detect()
            .map(capture_status)
            .is_ok_and(|status| status.enabled || status.active);
        SettingState::Toggle { enabled }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let backend = ClipBackend::detect()?;
        let status = capture_status(backend);

        if status.enabled || status.active {
            disable_capture()?;
            ctx.notify("Clipboard manager", "Clipboard history disabled");
        } else if enable_capture(backend)? {
            ctx.notify("Clipboard manager", "Clipboard history enabled");
        } else {
            ctx.emit_info(
                "settings.clipboard.aborted",
                "Clipboard history setup was cancelled.",
            );
        }

        Ok(())
    }
}
