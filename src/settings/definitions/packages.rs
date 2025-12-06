//! Package installer settings
//!
//! Interactive package browser and installer using pacman/AUR.

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::packages;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// Install Packages
// ============================================================================

pub struct InstallPackages;

impl Setting for InstallPackages {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.install_packages")
            .title("Install packages")
            .icon(NerdFont::Download)
            .summary("Browse and install system packages using an interactive fuzzy finder.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        packages::run_package_installer_action(ctx)
    }
}

inventory::submit! { &InstallPackages as &'static dyn Setting }
