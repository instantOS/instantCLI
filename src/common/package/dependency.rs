//! Dependency - a requirement that can be satisfied by one of multiple packages.

use super::PackageDefinition;
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

    /// Generate install hints for manual installation.
    ///
    /// Returns a string listing all possible installation methods.
    /// On immutable systems, filters out native package managers (pacman, apt, dnf, etc.)
    /// and only shows universal managers like Flatpak.
    pub fn install_hint(&self) -> String {
        let os = OperatingSystem::detect();
        let is_immutable = os.is_immutable();

        let hints: Vec<String> = self
            .packages
            .iter()
            .filter(|p| {
                // On immutable systems, only show universal package managers
                if is_immutable {
                    p.manager.is_universal()
                } else {
                    true
                }
            })
            .map(|p| p.install_hint())
            .collect();

        if hints.is_empty() {
            // When hints are empty on immutable systems, return a simple message
            // The UI layer will provide full context about immutable systems
            format!("Install `{}`", self.name)
        } else {
            format!("Try one of:\n{}", hints.join("\n"))
        }
    }

    /// Ensure this dependency is installed, prompting the user if needed.
    ///
    /// For installing multiple dependencies at once with a single prompt,
    /// use [`ensure_all`] instead.
    pub fn ensure(&'static self) -> anyhow::Result<InstallResult> {
        ensure_all(&[self])
    }
}

/// Ensure multiple dependencies are installed with a single confirmation prompt.
///
/// Groups packages by manager and installs them efficiently in batches.
/// Only shows one prompt for all packages.
///
/// # Example
///
/// ```ignore
/// use crate::common::package::{ensure_all, InstallResult};
///
/// match ensure_all(&[&FIREFOX, &PLAYERCTL, &MPV])? {
///     InstallResult::Installed | InstallResult::AlreadyInstalled => {
///         // All dependencies ready
///     }
///     InstallResult::Declined => return Ok(()),
///     InstallResult::NotAvailable { name, hint } => {
///         eprintln!("{} is not available: {}", name, hint);
///     }
///     InstallResult::Failed { reason } => {
///         eprintln!("Installation failed: {}", reason);
///     }
/// }
/// ```
pub fn ensure_all(deps: &[&'static Dependency]) -> anyhow::Result<InstallResult> {
    use super::batch::InstallBatch;

    // Check if all already installed
    if deps.iter().all(|d| d.is_installed()) {
        return Ok(InstallResult::AlreadyInstalled);
    }

    // Check if running on an immutable system
    let os = OperatingSystem::detect();
    if os.is_immutable() {
        let missing_deps: Vec<_> = deps
            .iter()
            .filter(|d| !d.is_installed())
            .map(|d| (d.name, d.install_hint()))
            .collect();

        if missing_deps.is_empty() {
            return Ok(InstallResult::AlreadyInstalled);
        }

        let (name, hint) = missing_deps.first().unwrap();
        // Just return the hint - the UI layer will add context about immutable systems
        return Ok(InstallResult::NotAvailable {
            name: name.to_string(),
            hint: hint.clone(),
        });
    }

    // Build the batch
    let mut batch = InstallBatch::new();
    let mut not_available = Vec::new();

    for dep in deps {
        if dep.is_installed() {
            continue;
        }
        if dep.get_best_package().is_none() {
            not_available.push((dep.name, dep.install_hint()));
        } else {
            // Safe because we have static lifetime
            batch.add(dep)?;
        }
    }

    // Report unavailable packages
    if !not_available.is_empty() {
        let (name, hint) = not_available.first().unwrap();
        if batch.is_empty() {
            return Ok(InstallResult::NotAvailable {
                name: name.to_string(),
                hint: hint.clone(),
            });
        }
        // Show warning but continue with installable packages
        crate::menu_utils::FzfWrapper::builder()
            .message(format!(
                "Some packages are unavailable:\n{}",
                not_available
                    .iter()
                    .map(|(n, _)| format!("  • {}", n))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
            .title("Warning")
            .show_message()?;
    }

    if batch.is_empty() {
        return Ok(InstallResult::AlreadyInstalled);
    }

    // Single prompt for all packages
    if !batch.prompt_confirmation()? {
        return Ok(InstallResult::Declined);
    }

    // Execute batched installation
    match batch.execute() {
        Ok(_) => {
            // Verify installation
            let all_installed = deps
                .iter()
                .all(|d| d.is_installed() || d.get_best_package().is_none());
            if all_installed {
                Ok(InstallResult::Installed)
            } else {
                Ok(InstallResult::Failed {
                    reason: "Some packages failed to install".to_string(),
                })
            }
        }
        Err(e) => Ok(InstallResult::Failed {
            reason: e.to_string(),
        }),
    }
}

/// Result of attempting to install dependencies.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallResult {
    /// All dependencies were already installed.
    AlreadyInstalled,
    /// All dependencies were successfully installed.
    Installed,
    /// User declined the installation.
    Declined,
    /// No package available for this system.
    NotAvailable { name: String, hint: String },
    /// Installation failed.
    Failed { reason: String },
}

impl InstallResult {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::AlreadyInstalled | Self::Installed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::package::PackageManager;

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
}
