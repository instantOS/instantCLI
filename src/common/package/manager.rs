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
    /// Pkg - Termux
    Pkg,

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
        matches!(
            self,
            Self::Pacman | Self::Apt | Self::Dnf | Self::Zypper | Self::Pkg
        )
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
            Self::Pacman | Self::Apt | Self::Dnf | Self::Zypper | Self::Pkg => 0,
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
            Self::Pacman | Self::Apt | Self::Dnf | Self::Zypper | Self::Pkg => {
                OperatingSystem::detect().native_package_manager() == Some(*self)
            }

            // Cross-platform managers - check binary availability
            Self::Flatpak => which::which("flatpak").is_ok(),
            Self::Aur => {
                OperatingSystem::detect().in_family(&OperatingSystem::Arch)
                    && detect_aur_helper().is_some()
            }
            Self::Cargo => which::which("cargo").is_ok(),
            Self::Snap => which::which("snap").is_ok(),
        }
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
            Self::Pkg => ("pkg", &["install", "-y"]),
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
            Self::Pkg => "Pkg",
            Self::Flatpak => "Flatpak",
            Self::Aur => "AUR",
            Self::Cargo => "Cargo",
            Self::Snap => "Snap",
        }
    }

    /// Get the lowercase identifier for this package manager.
    ///
    /// This is used for fzf source prefixes and serialization.
    /// Examples: "pacman", "apt", "snap", "aur"
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pacman => "pacman",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Zypper => "zypper",
            Self::Pkg => "pkg",
            Self::Flatpak => "flatpak",
            Self::Aur => "aur",
            Self::Cargo => "cargo",
            Self::Snap => "snap",
        }
    }

    /// Get the uninstall command for this package manager.
    ///
    /// Returns the command and base arguments used to uninstall packages.
    pub fn uninstall_command(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Pacman => ("sudo", &["pacman", "-R", "--noconfirm"]),
            Self::Apt => ("sudo", &["apt", "remove", "-y"]),
            Self::Dnf => ("sudo", &["dnf", "remove", "-y"]),
            Self::Zypper => ("sudo", &["zypper", "remove", "-y"]),
            Self::Pkg => ("pkg", &["uninstall", "-y"]),
            Self::Flatpak => ("flatpak", &["uninstall", "-y"]),
            Self::Aur => ("yay", &["-R", "--noconfirm"]),
            Self::Cargo => ("cargo", &["uninstall"]),
            Self::Snap => ("sudo", &["snap", "remove"]),
        }
    }

    /// Get the command to list available packages.
    ///
    /// Returns a shell command string that lists all available packages.
    pub fn list_available_command(&self) -> &'static str {
        match self {
            Self::Pacman => "pacman -Slq",
            Self::Apt => "apt-cache search . 2>/dev/null | grep -v '^$' | cut -d' ' -f1",
            Self::Dnf => "dnf list available 2>/dev/null | tail -n +2 | cut -d' ' -f1",
            Self::Zypper => {
                "zypper se --available-only 2>/dev/null | tail -n +3 | cut -d'|' -f2 | tr -d ' '"
            }
            Self::Pkg => "pkg list-all 2>/dev/null | cut -d'/' -f1",
            Self::Flatpak => "flatpak remote-ls --app --columns=application 2>/dev/null",
            Self::Aur => {
                "curl -sL https://aur.archlinux.org/packages.gz 2>/dev/null | gunzip 2>/dev/null"
            }
            Self::Cargo => "cargo search --limit 1000 '' 2>/dev/null | cut -d' ' -f1",
            Self::Snap => "snap find 2>/dev/null | tail -n +2 | cut -d' ' -f1",
        }
    }

    /// Get the command to list installed packages.
    ///
    /// Returns a shell command string that lists all installed packages.
    pub fn list_installed_command(&self) -> &'static str {
        match self {
            Self::Pacman => "pacman -Qq",
            Self::Apt => "dpkg-query -W -f='${Package}\\n' 2>/dev/null | sort",
            Self::Dnf => "dnf list installed 2>/dev/null | tail -n +2 | cut -d' ' -f1",
            Self::Zypper => {
                "zypper se --installed-only 2>/dev/null | tail -n +3 | cut -d'|' -f2 | tr -d ' '"
            }
            Self::Pkg => "pkg list-installed 2>/dev/null | cut -d'/' -f1",
            Self::Flatpak => "flatpak list --app --columns=application 2>/dev/null",
            Self::Aur => "pacman -Qm", // AUR packages are foreign packages in pacman
            Self::Cargo => "cargo install --list 2>/dev/null | grep -E '^[a-zA-Z]' | cut -d' ' -f1",
            Self::Snap => "snap list 2>/dev/null | tail -n +2 | cut -d' ' -f1",
        }
    }

    /// Get the command to show package information.
    ///
    /// Returns a shell command template with {package} placeholder.
    pub fn show_package_command(&self) -> &'static str {
        match self {
            Self::Pacman => "pacman -Qi {package}",
            Self::Apt => "apt show {package} 2>/dev/null",
            Self::Dnf => "dnf info {package} 2>/dev/null",
            Self::Zypper => "zypper info {package} 2>/dev/null",
            Self::Pkg => "pkg show {package} 2>/dev/null",
            Self::Flatpak => "flatpak info {package} 2>/dev/null",
            Self::Aur => "pacman -Qi {package}", // AUR packages use pacman for info
            Self::Cargo => "cargo show {package} 2>/dev/null",
            Self::Snap => "snap info {package} 2>/dev/null",
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl std::str::FromStr for PackageManager {
    type Err = String;

    /// Parse a package manager from its string identifier.
    ///
    /// This is used to map fzf source prefixes to package managers.
    /// Supports both lowercase identifiers and legacy "arch" for Pacman.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pacman" | "arch" | "repo" => Ok(PackageManager::Pacman),
            "apt" => Ok(PackageManager::Apt),
            "dnf" => Ok(PackageManager::Dnf),
            "zypper" => Ok(PackageManager::Zypper),
            "pkg" => Ok(PackageManager::Pkg),
            "flatpak" => Ok(PackageManager::Flatpak),
            "aur" => Ok(PackageManager::Aur),
            "cargo" => Ok(PackageManager::Cargo),
            "snap" => Ok(PackageManager::Snap),
            _ => Err(format!("Unknown package manager: {}", s)),
        }
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
        assert!(PackageManager::Snap.is_universal());
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
        assert_eq!(PackageManager::Snap.display_name(), "Snap");
    }

    #[test]
    fn test_snap_commands() {
        let (cmd, args) = PackageManager::Snap.install_command();
        assert_eq!(cmd, "sudo");
        assert_eq!(args, &["snap", "install"]);

        let (cmd, args) = PackageManager::Snap.uninstall_command();
        assert_eq!(cmd, "sudo");
        assert_eq!(args, &["snap", "remove"]);
    }

    #[test]
    fn test_from_str() {
        use std::str::FromStr;

        // Standard identifiers
        assert_eq!(
            PackageManager::from_str("pacman").unwrap(),
            PackageManager::Pacman
        );
        assert_eq!(
            PackageManager::from_str("apt").unwrap(),
            PackageManager::Apt
        );
        assert_eq!(
            PackageManager::from_str("dnf").unwrap(),
            PackageManager::Dnf
        );
        assert_eq!(
            PackageManager::from_str("zypper").unwrap(),
            PackageManager::Zypper
        );
        assert_eq!(
            PackageManager::from_str("pkg").unwrap(),
            PackageManager::Pkg
        );
        assert_eq!(
            PackageManager::from_str("flatpak").unwrap(),
            PackageManager::Flatpak
        );
        assert_eq!(
            PackageManager::from_str("aur").unwrap(),
            PackageManager::Aur
        );
        assert_eq!(
            PackageManager::from_str("cargo").unwrap(),
            PackageManager::Cargo
        );
        assert_eq!(
            PackageManager::from_str("snap").unwrap(),
            PackageManager::Snap
        );

        // Legacy/arch identifiers
        assert_eq!(
            PackageManager::from_str("arch").unwrap(),
            PackageManager::Pacman
        );
        assert_eq!(
            PackageManager::from_str("repo").unwrap(),
            PackageManager::Pacman
        );

        // Unknown identifier
        assert!(PackageManager::from_str("unknown").is_err());
    }

    #[test]
    fn test_as_str_roundtrip() {
        use std::str::FromStr;

        // Verify all managers can roundtrip through as_str() and from_str()
        let managers = [
            PackageManager::Pacman,
            PackageManager::Apt,
            PackageManager::Dnf,
            PackageManager::Zypper,
            PackageManager::Pkg,
            PackageManager::Flatpak,
            PackageManager::Aur,
            PackageManager::Cargo,
            PackageManager::Snap,
        ];

        for manager in managers {
            let s = manager.as_str();
            let parsed = PackageManager::from_str(s).unwrap();
            assert_eq!(manager, parsed, "Roundtrip failed for {:?}", manager);
        }
    }

    #[test]
    fn test_as_str_values() {
        assert_eq!(PackageManager::Pacman.as_str(), "pacman");
        assert_eq!(PackageManager::Apt.as_str(), "apt");
        assert_eq!(PackageManager::Dnf.as_str(), "dnf");
        assert_eq!(PackageManager::Zypper.as_str(), "zypper");
        assert_eq!(PackageManager::Pkg.as_str(), "pkg");
        assert_eq!(PackageManager::Flatpak.as_str(), "flatpak");
        assert_eq!(PackageManager::Aur.as_str(), "aur");
        assert_eq!(PackageManager::Cargo.as_str(), "cargo");
        assert_eq!(PackageManager::Snap.as_str(), "snap");
    }
}
