//! Dependency - a requirement that can be satisfied by one of multiple packages.

use super::{PackageDefinition, PackageManager};
use crate::common::distro::OperatingSystem;
use crate::common::requirements::InstallTest;

/// A unified dependency that can be satisfied by one of multiple packages.
///
/// A dependency represents something that needs to be installed (e.g., "Firefox browser").
/// It can be satisfied by any of its defined packages - the system will automatically
/// select the best one based on the current OS and package manager availability.
///
/// # Priority Selection
///
/// When selecting which package to install:
/// 1. Native package managers are tried first (Pacman, Apt, Dnf, Zypper)
/// 2. Then cross-platform managers in priority order (Flatpak → AUR → Cargo)
/// 3. Within the native manager, distro-specific packages take precedence
///
/// # Example
///
/// ```ignore
/// use crate::common::package::{Dependency, PackageDefinition, PackageManager};
/// use crate::common::requirements::InstallTest;
///
/// static FIREFOX: Dependency = Dependency {
///     name: "Firefox",
///     description: Some("Mozilla Firefox web browser"),
///     packages: &[
///         PackageDefinition::new("firefox", PackageManager::Pacman),
///         PackageDefinition::new("firefox", PackageManager::Apt),
///         PackageDefinition::new("org.mozilla.firefox", PackageManager::Flatpak),
///     ],
///     tests: &[InstallTest::WhichSucceeds("firefox")],
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Human-readable name for the dependency.
    pub name: &'static str,

    /// Description for UI display.
    pub description: Option<&'static str>,

    /// Available packages that can satisfy this dependency.
    ///
    /// The order matters for packages with the same manager - earlier packages
    /// in the list are preferred.
    pub packages: &'static [PackageDefinition],

    /// Tests to verify the dependency is satisfied.
    ///
    /// At least one test must pass for the dependency to be considered installed.
    pub tests: &'static [InstallTest],
}

impl Dependency {
    /// Check if this dependency is already installed.
    ///
    /// Returns `true` if any of the install tests pass.
    pub fn is_installed(&self) -> bool {
        self.tests.iter().any(|test| test.run())
    }

    /// Check if auto-install is available on the current system.
    ///
    /// Returns `true` if the dependency is already installed or if there's
    /// at least one package that can be automatically installed.
    pub fn can_auto_install(&self) -> bool {
        if self.is_installed() {
            return true;
        }
        self.get_best_package().is_some()
    }

    /// Get the best package to install for the current system.
    ///
    /// Returns `None` if no suitable package is found.
    ///
    /// Selection algorithm:
    /// 1. Try to find a package for the native package manager
    /// 2. If not found, try universal package managers in priority order
    pub fn get_best_package(&self) -> Option<&PackageDefinition> {
        let current_os = OperatingSystem::detect();
        let native_manager = current_os.native_package_manager();

        // First pass: try native package manager
        if let Some(manager) = native_manager {
            for pkg in self.packages {
                if pkg.manager == manager && pkg.applies_to(&current_os) {
                    return Some(pkg);
                }
            }
        }

        // Second pass: try universal package managers in priority order
        let mut universal_packages: Vec<_> = self
            .packages
            .iter()
            .filter(|p| p.manager.is_universal() && p.is_available())
            .collect();

        universal_packages.sort_by_key(|p| p.manager.priority());
        universal_packages.first().copied()
    }

    /// Get all packages that could work on the current system.
    ///
    /// Returns packages sorted by priority (best first).
    pub fn get_available_packages(&self) -> Vec<&PackageDefinition> {
        let current_os = OperatingSystem::detect();

        let mut available: Vec<_> = self
            .packages
            .iter()
            .filter(|p| p.is_available() && p.applies_to(&current_os))
            .collect();

        // Sort by priority (lower is better), then by position in original list
        available.sort_by_key(|p| p.manager.priority());
        available
    }

    /// Generate install hints for manual installation.
    ///
    /// Returns a string listing all possible installation methods.
    pub fn install_hint(&self) -> String {
        let hints: Vec<String> = self.packages.iter().map(|p| p.install_hint()).collect();

        if hints.is_empty() {
            format!("Install `{}`", self.name)
        } else {
            format!("Try one of:\n{}", hints.join("\n"))
        }
    }

    /// Get a concise list of available package managers for this dependency.
    ///
    /// This is useful for displaying to users which installation methods are available.
    pub fn available_managers(&self) -> Vec<PackageManager> {
        let current_os = OperatingSystem::detect();

        let mut managers: Vec<PackageManager> = self
            .packages
            .iter()
            .filter(|p| p.is_available() && p.applies_to(&current_os))
            .map(|p| p.manager)
            .collect();

        managers.sort_by_key(|m| m.priority());
        managers.dedup();
        managers
    }

    /// Ensure this dependency is installed, prompting the user if needed.
    ///
    /// This is the main entry point for dependency installation. It handles:
    /// - Checking if already installed
    /// - Finding the best package for the current system
    /// - Prompting for user confirmation
    /// - Executing the installation
    /// - Verifying the installation succeeded
    ///
    /// # Returns
    ///
    /// - `InstallResult::AlreadyInstalled` - Dependency was already satisfied
    /// - `InstallResult::Installed` - Successfully installed
    /// - `InstallResult::Declined` - User declined installation
    /// - `InstallResult::NotAvailable` - No package available for this system
    /// - `InstallResult::Failed` - Installation failed
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crate::common::package::InstallResult;
    ///
    /// match FIREFOX.ensure()? {
    ///     InstallResult::AlreadyInstalled | InstallResult::Installed => {
    ///         // Firefox is ready to use
    ///     }
    ///     InstallResult::Declined => {
    ///         // User cancelled, exit gracefully
    ///     }
    ///     InstallResult::NotAvailable { hint } => {
    ///         println!("Firefox not available. {}", hint);
    ///     }
    ///     InstallResult::Failed { reason } => {
    ///         eprintln!("Installation failed: {}", reason);
    ///     }
    /// }
    /// ```
    pub fn ensure(&self) -> anyhow::Result<InstallResult> {
        // Already installed - no action needed
        if self.is_installed() {
            return Ok(InstallResult::AlreadyInstalled);
        }

        // Find the best package for this system
        let pkg_def = match self.get_best_package() {
            Some(p) => p,
            None => {
                return Ok(InstallResult::NotAvailable {
                    name: self.name.to_string(),
                    hint: self.install_hint(),
                });
            }
        };

        // Prompt for confirmation
        let msg = format!(
            "Install {} via {}?\n\n{}",
            self.name,
            pkg_def.manager.display_name(),
            pkg_def.install_hint()
        );

        let should_install = crate::menu_utils::FzfWrapper::builder()
            .confirm(&msg)
            .yes_text("Install")
            .no_text("Cancel")
            .show_confirmation()?;

        if !matches!(should_install, crate::menu_utils::ConfirmResult::Yes) {
            return Ok(InstallResult::Declined);
        }

        // Execute installation
        self.install_package(pkg_def)?;

        // Verify installation
        if self.is_installed() {
            Ok(InstallResult::Installed)
        } else {
            Ok(InstallResult::Failed {
                reason: format!(
                    "Installation completed but {} is still not detected",
                    self.name
                ),
            })
        }
    }

    /// Install a specific package definition (internal helper).
    fn install_package(&self, pkg_def: &PackageDefinition) -> anyhow::Result<()> {
        use anyhow::Context;

        let (cmd, base_args) = pkg_def.manager.install_command();
        let mut args: Vec<&str> = base_args.to_vec();
        args.push(pkg_def.package_name);

        // Handle AUR helper detection
        let actual_cmd = if pkg_def.manager == PackageManager::Aur {
            super::detect_aur_helper().unwrap_or("yay")
        } else {
            cmd
        };

        duct::cmd(actual_cmd, &args)
            .run()
            .with_context(|| {
                format!(
                    "Failed to install {} via {}",
                    self.name,
                    pkg_def.manager.display_name()
                )
            })?;

        Ok(())
    }
}

/// Result of attempting to ensure a dependency is installed.
///
/// This enum covers all possible outcomes of a dependency installation attempt,
/// making it easy for consumers to handle each case appropriately.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallResult {
    /// Dependency was already installed before we checked.
    AlreadyInstalled,

    /// Dependency was successfully installed.
    Installed,

    /// User declined the installation prompt.
    Declined,

    /// No package is available for this dependency on the current system.
    ///
    /// This happens when:
    /// - The dependency has no packages defined for the current OS
    /// - No package manager is available that can install it
    NotAvailable {
        /// Name of the dependency
        name: String,
        /// Hint for manual installation
        hint: String,
    },

    /// Installation was attempted but failed.
    Failed {
        /// Reason for the failure
        reason: String,
    },
}

impl InstallResult {
    /// Returns true if the dependency is now available for use.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::AlreadyInstalled | Self::Installed)
    }

    /// Returns true if installation was declined by the user.
    pub fn is_declined(&self) -> bool {
        matches!(self, Self::Declined)
    }

    /// Returns true if installation failed or wasn't possible.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::NotAvailable { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Static test data - must be 'static for Dependency struct
    static TEST_PACKAGES: &[PackageDefinition] = &[
        PackageDefinition {
            package_name: "test-pkg",
            manager: PackageManager::Pacman,
            distros: None,
        },
        PackageDefinition {
            package_name: "test-pkg",
            manager: PackageManager::Apt,
            distros: None,
        },
        PackageDefinition {
            package_name: "test-pkg",
            manager: PackageManager::Flatpak,
            distros: None,
        },
    ];

    static TEST_TESTS: &[InstallTest] = &[InstallTest::WhichSucceeds("test-pkg-binary")];

    static TEST_DEPENDENCY: Dependency = Dependency {
        name: "test-pkg",
        description: Some("A test package"),
        packages: TEST_PACKAGES,
        tests: TEST_TESTS,
    };

    #[test]
    fn test_install_hint() {
        let hint = TEST_DEPENDENCY.install_hint();
        assert!(hint.contains("pacman -S test-pkg"));
        assert!(hint.contains("apt install test-pkg"));
        assert!(hint.contains("flatpak install flathub test-pkg"));
    }

    static EMPTY_PACKAGES: &[PackageDefinition] = &[];
    static NONEXISTENT_TESTS: &[InstallTest] =
        &[InstallTest::WhichSucceeds("definitely-does-not-exist-12345")];

    static NONEXISTENT_DEPENDENCY: Dependency = Dependency {
        name: "nonexistent",
        description: None,
        packages: EMPTY_PACKAGES,
        tests: NONEXISTENT_TESTS,
    };

    #[test]
    fn test_can_auto_install_when_not_installed() {
        // No packages available, so can't auto-install
        assert!(!NONEXISTENT_DEPENDENCY.can_auto_install());
    }
}
