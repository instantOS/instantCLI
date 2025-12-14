//! Package manager enum and related functionality.

use crate::common::distro::OperatingSystem;

/// Represents how a package is installed - SINGLE SOURCE OF TRUTH for package managers.
///
/// This enum replaces the old `PackageManager` enum in `requirements.rs` and adds
/// support for cross-platform package managers like Flatpak, AUR, and Cargo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageManager {
    // =========================================================================
    // Native distro package managers (highest priority - tried first)
    // =========================================================================
    /// Pacman - Arch Linux family
    Pacman,
    /// APT - Debian/Ubuntu family
    Apt,
    /// DNF - Fedora/RHEL family
    Dnf,
    /// Zypper - OpenSUSE
    Zypper,

    // =========================================================================
    // Cross-platform / secondary package managers (tried as fallback)
    // Priority order: Flatpak (prebuilt) → AUR (compiles) → Cargo (compiles)
    // =========================================================================
    /// Flatpak - prebuilt, sandboxed, low resource usage
    Flatpak,
    /// AUR (Arch User Repository) - compiles from source
    Aur,
    /// Cargo - Rust packages, compiles from source, most resource intensive
    Cargo,
    /// Snap - Canonical's package format (future)
    Snap,
}

impl PackageManager {
    /// Returns true if this is a system/native package manager.
    ///
    /// Native package managers are the primary package management system for a distribution.
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Pacman | Self::Apt | Self::Dnf | Self::Zypper)
    }

    /// Returns true if this is a cross-platform/fallback package manager.
    ///
    /// Universal package managers work across multiple distributions.
    pub fn is_universal(&self) -> bool {
        !self.is_native()
    }

    /// Returns the priority for installation (lower = preferred).
    ///
    /// Priority is based on resource usage: prebuilt packages preferred over compiled ones.
    /// - 0: Native package managers (highest priority)
    /// - 1: Flatpak/Snap (prebuilt)
    /// - 2: AUR (compiles)
    /// - 3: Cargo (compiles, most resource intensive)
    pub fn priority(&self) -> u8 {
        match self {
            // Native package managers - highest priority (prebuilt packages)
            Self::Pacman | Self::Apt | Self::Dnf | Self::Zypper => 0,
            // Flatpak/Snap - prebuilt, sandboxed, low resource usage
            Self::Flatpak | Self::Snap => 1,
            // AUR - compiles from source, moderate resource usage
            Self::Aur => 2,
            // Cargo - compiles from source, highest resource usage
            Self::Cargo => 3,
        }
    }

    /// Check if this manager is available on the current system.
    ///
    /// For native managers, this delegates to `OperatingSystem::native_package_manager()`
    /// to avoid duplicating the OS → manager mapping logic.
    pub fn is_available(&self) -> bool {
        match self {
            // Native managers - delegate to OperatingSystem
            Self::Pacman | Self::Apt | Self::Dnf | Self::Zypper => {
                OperatingSystem::detect().native_package_manager() == Some(*self)
            }

            // Cross-platform managers - check binary availability
            Self::Flatpak => which::which("flatpak").is_ok(),
            Self::Aur => OperatingSystem::detect().is_arch_based() && detect_aur_helper().is_some(),
            Self::Cargo => which::which("cargo").is_ok(),
            Self::Snap => which::which("snap").is_ok(),
        }
    }

    /// Get the native package manager for the current system (convenience method).
    ///
    /// This is a shorthand for `OperatingSystem::detect().native_package_manager()`.
    pub fn native_for_current_os() -> Option<Self> {
        OperatingSystem::detect().native_package_manager()
    }

    /// Get the install command prefix for this package manager.
    ///
    /// Returns the command and base arguments used to install packages.
    pub fn install_command(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Pacman => ("sudo", &["pacman", "-S", "--noconfirm"]),
            Self::Apt => ("sudo", &["apt", "install", "-y"]),
            Self::Dnf => ("sudo", &["dnf", "install", "-y"]),
            Self::Zypper => ("sudo", &["zypper", "install", "-y"]),
            Self::Flatpak => ("flatpak", &["install", "-y", "flathub"]),
            Self::Aur => {
                // Will be handled specially to use detected AUR helper
                ("yay", &["-S", "--noconfirm"])
            }
            Self::Cargo => ("cargo", &["install"]),
            Self::Snap => ("sudo", &["snap", "install"]),
        }
    }

    /// Get a human-readable name for this package manager.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Pacman => "Pacman",
            Self::Apt => "APT",
            Self::Dnf => "DNF",
            Self::Zypper => "Zypper",
            Self::Flatpak => "Flatpak",
            Self::Aur => "AUR",
            Self::Cargo => "Cargo",
            Self::Snap => "Snap",
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Detect available AUR helper (yay, paru, etc.)
///
/// Returns the name of the first available AUR helper found.
pub fn detect_aur_helper() -> Option<&'static str> {
    const AUR_HELPERS: &[&str] = &["yay", "paru", "pikaur", "trizen"];

    AUR_HELPERS
        .iter()
        .find(|&helper| which::which(helper).is_ok())
        .map(|v| v as _)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_native() {
        assert!(PackageManager::Pacman.is_native());
        assert!(PackageManager::Apt.is_native());
        assert!(PackageManager::Dnf.is_native());
        assert!(PackageManager::Zypper.is_native());

        assert!(!PackageManager::Flatpak.is_native());
        assert!(!PackageManager::Aur.is_native());
        assert!(!PackageManager::Cargo.is_native());
        assert!(!PackageManager::Snap.is_native());
    }

    #[test]
    fn test_is_universal() {
        assert!(!PackageManager::Pacman.is_universal());
        assert!(PackageManager::Flatpak.is_universal());
        assert!(PackageManager::Cargo.is_universal());
    }

    #[test]
    fn test_priority() {
        // Native managers have highest priority (0)
        assert_eq!(PackageManager::Pacman.priority(), 0);
        assert_eq!(PackageManager::Apt.priority(), 0);

        // Flatpak/Snap next (1)
        assert_eq!(PackageManager::Flatpak.priority(), 1);
        assert_eq!(PackageManager::Snap.priority(), 1);

        // AUR (2)
        assert_eq!(PackageManager::Aur.priority(), 2);

        // Cargo last (3)
        assert_eq!(PackageManager::Cargo.priority(), 3);
    }

    #[test]
    fn test_display_name() {
        assert_eq!(PackageManager::Pacman.display_name(), "Pacman");
        assert_eq!(PackageManager::Apt.display_name(), "APT");
        assert_eq!(PackageManager::Flatpak.display_name(), "Flatpak");
    }
}
