//! Legacy compatibility and migration helpers.
//!
//! This module provides conversions from the old `RequiredPackage` and
//! `FlatpakPackage` types to the new `Dependency` format.

use super::{Dependency, PackageDefinition, PackageManager};
use crate::common::requirements::{FlatpakPackage, RequiredPackage};

/// Convert a `RequiredPackage` to a `Dependency`.
///
/// This creates a `Dependency` with packages for Pacman and Apt based on
/// the `arch_package_name` and `ubuntu_package_name` fields.
impl RequiredPackage {
    /// Convert this `RequiredPackage` to the new `Dependency` format.
    ///
    /// Note: This is a runtime conversion and creates heap allocations.
    /// For static definitions, use the new `Dependency` format directly.
    pub fn to_dependency(&self) -> Dependency {
        // Collect package definitions based on available package names
        let mut packages = Vec::new();

        if let Some(arch_name) = self.arch_package_name {
            packages.push(PackageDefinition {
                package_name: arch_name,
                manager: PackageManager::Pacman,
                distros: None,
            });
        }

        if let Some(ubuntu_name) = self.ubuntu_package_name {
            packages.push(PackageDefinition {
                package_name: ubuntu_name,
                manager: PackageManager::Apt,
                distros: None,
            });
        }

        // Leak the vec to get 'static lifetime - acceptable for migration
        let packages: &'static [PackageDefinition] = Box::leak(packages.into_boxed_slice());

        Dependency {
            name: self.name,
            description: None,
            packages,
            tests: self.tests,
        }
    }
}

/// Convert a `FlatpakPackage` to a `Dependency`.
impl FlatpakPackage {
    /// Convert this `FlatpakPackage` to the new `Dependency` format.
    pub fn to_dependency(&self) -> Dependency {
        let packages: &'static [PackageDefinition] = Box::leak(
            vec![PackageDefinition {
                package_name: self.app_id,
                manager: PackageManager::Flatpak,
                distros: None,
            }]
            .into_boxed_slice(),
        );

        Dependency {
            name: self.name,
            description: None,
            packages,
            tests: self.tests,
        }
    }
}

/// Macro to create a `Dependency` with minimal boilerplate.
///
/// # Examples
///
/// ```ignore
/// // Simple package with same name on Pacman and Apt
/// dep!(FIREFOX, "Firefox", "firefox");
///
/// // Package with different names
/// dep!(CHROMIUM, "Chromium", pacman: "chromium", apt: "chromium-browser");
///
/// // With description
/// dep!(VLC, "VLC", "vlc", "Universal media player");
///
/// // With Flatpak fallback
/// dep!(FIREFOX, "Firefox", "firefox", flatpak: "org.mozilla.firefox");
///
/// // With custom test
/// dep!(LIBNOTIFY, "libnotify", pacman: "libnotify", apt: "libnotify-bin", test: which("notify-send"));
/// ```
#[macro_export]
macro_rules! dep {
    // Simple: same package name for pacman and apt
    ($name:ident, $display:expr, $pkg:expr) => {
        pub static $name: $crate::common::package::Dependency =
            $crate::common::package::Dependency {
                name: $display,
                description: None,
                packages: &[
                    $crate::common::package::PackageDefinition {
                        package_name: $pkg,
                        manager: $crate::common::package::PackageManager::Pacman,
                        distros: None,
                    },
                    $crate::common::package::PackageDefinition {
                        package_name: $pkg,
                        manager: $crate::common::package::PackageManager::Apt,
                        distros: None,
                    },
                ],
                tests: &[$crate::common::requirements::InstallTest::WhichSucceeds(
                    $pkg,
                )],
            };
    };

    // With description
    ($name:ident, $display:expr, $pkg:expr, $desc:expr) => {
        pub static $name: $crate::common::package::Dependency =
            $crate::common::package::Dependency {
                name: $display,
                description: Some($desc),
                packages: &[
                    $crate::common::package::PackageDefinition {
                        package_name: $pkg,
                        manager: $crate::common::package::PackageManager::Pacman,
                        distros: None,
                    },
                    $crate::common::package::PackageDefinition {
                        package_name: $pkg,
                        manager: $crate::common::package::PackageManager::Apt,
                        distros: None,
                    },
                ],
                tests: &[$crate::common::requirements::InstallTest::WhichSucceeds(
                    $pkg,
                )],
            };
    };

    // Different package names for pacman and apt
    ($name:ident, $display:expr, pacman: $pacman:expr, apt: $apt:expr) => {
        pub static $name: $crate::common::package::Dependency =
            $crate::common::package::Dependency {
                name: $display,
                description: None,
                packages: &[
                    $crate::common::package::PackageDefinition {
                        package_name: $pacman,
                        manager: $crate::common::package::PackageManager::Pacman,
                        distros: None,
                    },
                    $crate::common::package::PackageDefinition {
                        package_name: $apt,
                        manager: $crate::common::package::PackageManager::Apt,
                        distros: None,
                    },
                ],
                tests: &[$crate::common::requirements::InstallTest::WhichSucceeds(
                    $pacman,
                )],
            };
    };

    // Different package names with custom binary test
    ($name:ident, $display:expr, pacman: $pacman:expr, apt: $apt:expr, test: $bin:expr) => {
        pub static $name: $crate::common::package::Dependency =
            $crate::common::package::Dependency {
                name: $display,
                description: None,
                packages: &[
                    $crate::common::package::PackageDefinition {
                        package_name: $pacman,
                        manager: $crate::common::package::PackageManager::Pacman,
                        distros: None,
                    },
                    $crate::common::package::PackageDefinition {
                        package_name: $apt,
                        manager: $crate::common::package::PackageManager::Apt,
                        distros: None,
                    },
                ],
                tests: &[$crate::common::requirements::InstallTest::WhichSucceeds(
                    $bin,
                )],
            };
    };

    // With Flatpak fallback
    ($name:ident, $display:expr, $pkg:expr, flatpak: $flatpak:expr) => {
        pub static $name: $crate::common::package::Dependency =
            $crate::common::package::Dependency {
                name: $display,
                description: None,
                packages: &[
                    $crate::common::package::PackageDefinition {
                        package_name: $pkg,
                        manager: $crate::common::package::PackageManager::Pacman,
                        distros: None,
                    },
                    $crate::common::package::PackageDefinition {
                        package_name: $pkg,
                        manager: $crate::common::package::PackageManager::Apt,
                        distros: None,
                    },
                    $crate::common::package::PackageDefinition {
                        package_name: $flatpak,
                        manager: $crate::common::package::PackageManager::Flatpak,
                        distros: None,
                    },
                ],
                tests: &[$crate::common::requirements::InstallTest::WhichSucceeds(
                    $pkg,
                )],
            };
    };

    // Pacman only with Cargo fallback
    ($name:ident, $display:expr, pacman: $pacman:expr, cargo: $cargo:expr) => {
        pub static $name: $crate::common::package::Dependency =
            $crate::common::package::Dependency {
                name: $display,
                description: None,
                packages: &[
                    $crate::common::package::PackageDefinition {
                        package_name: $pacman,
                        manager: $crate::common::package::PackageManager::Pacman,
                        distros: None,
                    },
                    $crate::common::package::PackageDefinition {
                        package_name: $cargo,
                        manager: $crate::common::package::PackageManager::Cargo,
                        distros: None,
                    },
                ],
                tests: &[$crate::common::requirements::InstallTest::WhichSucceeds(
                    $pacman,
                )],
            };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_package_to_dependency() {
        let pkg = RequiredPackage {
            name: "test",
            arch_package_name: Some("test-arch"),
            ubuntu_package_name: Some("test-ubuntu"),
            tests: &[InstallTest::WhichSucceeds("test-bin")],
        };

        let dep = pkg.to_dependency();
        assert_eq!(dep.name, "test");
        assert_eq!(dep.packages.len(), 2);
        assert_eq!(dep.packages[0].package_name, "test-arch");
        assert_eq!(dep.packages[0].manager, PackageManager::Pacman);
        assert_eq!(dep.packages[1].package_name, "test-ubuntu");
        assert_eq!(dep.packages[1].manager, PackageManager::Apt);
    }

    #[test]
    fn test_flatpak_package_to_dependency() {
        let pkg = FlatpakPackage {
            name: "Test App",
            app_id: "com.test.App",
            tests: &[InstallTest::CommandSucceeds {
                program: "flatpak",
                args: &["info", "com.test.App"],
            }],
        };

        let dep = pkg.to_dependency();
        assert_eq!(dep.name, "Test App");
        assert_eq!(dep.packages.len(), 1);
        assert_eq!(dep.packages[0].package_name, "com.test.App");
        assert_eq!(dep.packages[0].manager, PackageManager::Flatpak);
    }
}
