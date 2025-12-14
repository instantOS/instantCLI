//! Macros for defining packages and dependencies.

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
