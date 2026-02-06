//! Manage installed packages setting
//!
//! Interactive package browser and uninstaller for installed system packages.

use anyhow::Result;

use crate::common::distro::OperatingSystem;
use crate::settings::context::SettingsContext;
use crate::settings::installed_packages_manager;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

/// Manage installed packages setting.
///
/// This setting allows users to view and uninstall installed system packages.
/// Currently supports Debian-based (apt/pkg) and Arch-based (pacman/AUR) systems.
pub struct ManageInstalledPackages;

impl Setting for ManageInstalledPackages {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.manage_installed_packages")
            .title("Manage installed packages")
            .icon(NerdFont::Package)
            .summary("View and uninstall installed system packages.")
            .supported_distros(&[
                OperatingSystem::Arch,
                OperatingSystem::Debian,
                OperatingSystem::Fedora,
            ])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        installed_packages_manager::run_installed_packages_manager(ctx.debug())
    }
}
