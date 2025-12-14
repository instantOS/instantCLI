//! Legacy package requirements types.
//!
//! This module contains types that are being phased out in favor of the new
//! unified `Dependency` system in `crate::common::package`.
//!
//! The only actively used exports are:
//! - `InstallTest` - Test conditions for verifying package installation
//! - `PackageStatus` - Result status of installation attempts
//!
//! The remaining types (`RequiredPackage`, `FlatpakPackage`, `PackageManager`)
//! are kept for compatibility with `legacy.rs` conversions.

use std::path::Path;

use crate::menu_utils::FzfWrapper;
use anyhow::{Context, Result};
use duct::cmd;

/// Status of a package installation request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageStatus {
    /// Package is installed and ready to use
    Installed,
    /// User explicitly declined installation
    Declined,
    /// Installation failed or verification failed
    Failed,
}

impl PackageStatus {
    /// Check if the package is effectively installed
    pub fn is_installed(&self) -> bool {
        matches!(self, Self::Installed)
    }
}

/// Tests for determining whether a dependency is available on the system.
#[derive(Debug, Clone, Copy)]
pub enum InstallTest {
    /// Succeeds when `which <program>` resolves.
    WhichSucceeds(&'static str),
    /// Succeeds when the given path exists.
    FileExists(&'static str),
    /// Succeeds when the command exits with status 0.
    CommandSucceeds {
        program: &'static str,
        args: &'static [&'static str],
    },
}

impl InstallTest {
    pub fn run(self) -> bool {
        match self {
            InstallTest::WhichSucceeds(program) => which::which(program).is_ok(),
            InstallTest::FileExists(path) => Path::new(path).exists(),
            InstallTest::CommandSucceeds { program, args } => cmd(program, args).run().is_ok(),
        }
    }
}

// =============================================================================
// Legacy Types (for backward compatibility with legacy.rs)
// =============================================================================

/// Represents an external dependency a setting may require.
/// DEPRECATED: Use `crate::common::package::Dependency` instead.
#[derive(Debug, Clone, Copy)]
pub struct RequiredPackage {
    pub name: &'static str,
    pub arch_package_name: Option<&'static str>,
    pub ubuntu_package_name: Option<&'static str>,
    pub tests: &'static [InstallTest],
}

/// Represents a Flatpak application dependency
/// DEPRECATED: Use `crate::common::package::Dependency` with `PackageManager::Flatpak` instead.
#[derive(Debug, Clone, Copy)]
pub struct FlatpakPackage {
    pub name: &'static str,
    pub app_id: &'static str,
    pub tests: &'static [InstallTest],
}

/// Legacy package manager enum used by distro detection.
/// Note: This is different from `crate::common::package::PackageManager`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageManager {
    Pacman,
    Apt,
}

impl PackageManager {
    /// Detect the available package manager on the system
    pub fn detect() -> Option<Self> {
        crate::common::distro::OperatingSystem::detect().package_manager()
    }

    /// Get the package name for this package manager
    pub fn package_name(&self, package: &RequiredPackage) -> Option<&'static str> {
        match self {
            PackageManager::Pacman => package.arch_package_name,
            PackageManager::Apt => package.ubuntu_package_name,
        }
    }

    /// Install a package using this package manager
    pub fn install_package(&self, package: &RequiredPackage) -> Result<()> {
        let package_name = self
            .package_name(package)
            .ok_or_else(|| anyhow::anyhow!("No package name available for this package manager"))?;

        match self {
            PackageManager::Pacman => {
                cmd!("sudo", "pacman", "-S", package_name)
                    .run()
                    .with_context(|| format!("Failed to install {} with pacman", package.name))?;
            }
            PackageManager::Apt => {
                cmd!("sudo", "apt", "install", package_name)
                    .run()
                    .with_context(|| format!("Failed to install {} with apt", package.name))?;
            }
        }
        Ok(())
    }
}

impl RequiredPackage {
    pub fn is_installed(&self) -> bool {
        self.tests.iter().any(|test| test.run())
    }

    /// Check if this package can be automatically installed on the current system.
    pub fn can_auto_install(&self) -> bool {
        if self.is_installed() {
            return true;
        }
        PackageManager::detect()
            .and_then(|pm| pm.package_name(self))
            .is_some()
    }

    fn install_hint(&self) -> String {
        let manual = format!(
            "Package names:\n  - Arch: {}\n  - Ubuntu: {}",
            self.arch_package_name.unwrap_or("not available"),
            self.ubuntu_package_name.unwrap_or("not available")
        );
        manual
    }

    /// Ensure the package is installed, prompting for installation if needed
    pub fn ensure(&self) -> Result<PackageStatus> {
        if self.is_installed() {
            return Ok(PackageStatus::Installed);
        }

        // Check if we can auto-install this package
        if !self.can_auto_install() {
            let msg = format!(
                "The required package '{}' must be installed manually.\n\n{}",
                self.name,
                self.install_hint()
            );
            FzfWrapper::builder()
                .message(&msg)
                .title("Manual Installation Required")
                .show_message()?;
            return Ok(PackageStatus::Failed);
        }

        // Package is not installed, prompt for installation
        let install_msg = format!(
            "The required package '{}' is not installed.\n\nDo you want to install it?",
            self.name
        );

        let should_install = FzfWrapper::builder()
            .confirm(&install_msg)
            .yes_text("Install")
            .no_text("Cancel")
            .show_confirmation()?;

        let should_install = matches!(should_install, crate::menu_utils::ConfirmResult::Yes);

        if should_install {
            if let Err(err) = self.install_with_prompt() {
                let error_msg = format!(
                    "Failed to install '{}': {}\n\nThis package is required for the selected setting.\n\n{}",
                    self.name,
                    err,
                    self.install_hint()
                );
                FzfWrapper::builder()
                    .message(&error_msg)
                    .title("Installation Failed")
                    .show_message()?;
                return Ok(PackageStatus::Failed);
            }
            Ok(PackageStatus::Installed)
        } else {
            Ok(PackageStatus::Declined)
        }
    }

    /// Attempt to install the package
    fn install_with_prompt(&self) -> Result<()> {
        let package_manager = PackageManager::detect()
            .ok_or_else(|| anyhow::anyhow!("No supported package manager found"))?;

        let package_name = package_manager
            .package_name(self)
            .ok_or_else(|| anyhow::anyhow!("Package not available for this system"))?;

        let installing_msg = format!("Installing {} (package: {})...", self.name, package_name);

        FzfWrapper::builder()
            .message(&installing_msg)
            .title("Installing Package")
            .show_message()?;

        package_manager.install_package(self)?;

        if !self.is_installed() {
            return Err(anyhow::anyhow!(
                "Installation of '{}' completed but verification checks still failed",
                self.name
            ));
        }

        let success_msg = format!("Successfully installed {}!", self.name);
        FzfWrapper::builder()
            .message(&success_msg)
            .title("Installation Complete")
            .show_message()?;

        Ok(())
    }
}

impl FlatpakPackage {
    pub fn is_installed(&self) -> bool {
        self.tests.iter().any(|test| test.run())
    }

    fn is_flathub_configured() -> bool {
        cmd!("flatpak", "remote-list")
            .read()
            .map(|output| output.contains("flathub"))
            .unwrap_or(false)
    }

    fn setup_flathub() -> Result<()> {
        let msg = "Flathub is not configured. Setting up Flathub remote...";
        FzfWrapper::builder()
            .message(msg)
            .title("Flatpak Setup")
            .show_message()?;

        cmd!(
            "flatpak",
            "remote-add",
            "--if-not-exists",
            "flathub",
            "https://dl.flathub.org/repo/flathub.flatpakrepo"
        )
        .run()
        .context("Failed to add Flathub remote")?;

        Ok(())
    }

    fn install_from_flathub(&self) -> Result<()> {
        if !Self::is_flathub_configured() {
            Self::setup_flathub()?;
        }

        let installing_msg = format!("Installing {} from Flathub...", self.name);
        FzfWrapper::builder()
            .message(&installing_msg)
            .title("Installing Flatpak")
            .show_message()?;

        cmd!("flatpak", "install", "-y", "flathub", self.app_id)
            .run()
            .with_context(|| format!("Failed to install {} from Flathub", self.name))?;

        if !self.is_installed() {
            return Err(anyhow::anyhow!(
                "Installation of '{}' completed but verification checks still failed",
                self.name
            ));
        }

        let success_msg = format!("Successfully installed {} from Flathub!", self.name);
        FzfWrapper::builder()
            .message(&success_msg)
            .title("Installation Complete")
            .show_message()?;

        Ok(())
    }

    pub fn ensure(&self) -> Result<PackageStatus> {
        if self.is_installed() {
            return Ok(PackageStatus::Installed);
        }

        let install_msg = format!(
            "The Flatpak application '{}' is not installed.\n\nDo you want to install it from Flathub?",
            self.name
        );

        let should_install = FzfWrapper::builder()
            .confirm(&install_msg)
            .yes_text("Install")
            .no_text("Cancel")
            .show_confirmation()?;

        let should_install = matches!(should_install, crate::menu_utils::ConfirmResult::Yes);

        if should_install {
            if let Err(err) = self.install_from_flathub() {
                let error_msg = format!(
                    "Failed to install '{}': {}\n\nManual installation:\nflatpak install flathub {}",
                    self.name, err, self.app_id
                );
                FzfWrapper::builder()
                    .message(&error_msg)
                    .title("Installation Failed")
                    .show_message()?;
                return Ok(PackageStatus::Failed);
            }
            Ok(PackageStatus::Installed)
        } else {
            Ok(PackageStatus::Declined)
        }
    }
}
