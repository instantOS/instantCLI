//! Package installer settings
//!
//! Interactive package browser and installer supporting multiple package managers.

use anyhow::Result;

use crate::common::distro::OperatingSystem;
use crate::settings::context::SettingsContext;
use crate::settings::packages;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// Install Packages (Distro-agnostic)
// ============================================================================

/// Install packages setting that supports multiple package managers.
///
/// Automatically detects the distribution and uses the appropriate package manager:
/// - Arch-based: pacman + AUR helpers
/// - Debian-based: apt (or pkg on Termux)
/// - Fedora-based: dnf
/// - openSUSE: zypper
pub struct InstallPackages;

impl Setting for InstallPackages {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.install_packages")
            .title("Install packages")
            .icon(NerdFont::Download)
            .summary("Browse and install system packages using an interactive fuzzy finder.")
            .supported_distros(&[
                OperatingSystem::Arch,
                OperatingSystem::Debian,
                OperatingSystem::Fedora,
                OperatingSystem::OpenSUSE,
            ])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        packages::run_package_installer_action(ctx)
    }
}
