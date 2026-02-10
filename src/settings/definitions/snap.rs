//! Snap installer settings
//!
//! Interactive Snap app browser and installer.

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::packages;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// Install Snap Apps
// ============================================================================

pub struct InstallSnapApps;

impl Setting for InstallSnapApps {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.install_snap_apps")
            .title("Install Snap apps")
            .icon(NerdFont::Download)
            .summary("Browse and install Snap applications using an interactive fuzzy finder.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        packages::run_snap_installer(ctx.debug())
    }
}
