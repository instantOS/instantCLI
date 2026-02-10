//! Manage installed Snap apps setting
//!
//! Interactive Snap browser and uninstaller for installed Snap applications.

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::installed_packages_manager;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

/// Manage installed Snap apps setting.
///
/// This setting allows users to view and uninstall installed Snap applications.
pub struct ManageInstalledSnaps;

impl Setting for ManageInstalledSnaps {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.manage_installed_snaps")
            .title("Manage installed Snaps")
            .icon(NerdFont::Package)
            .summary("View and uninstall installed Snap applications.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        installed_packages_manager::run_snap_uninstaller(ctx.debug())
    }
}
