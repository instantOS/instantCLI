//! Package definition - specifies how to install a package with a specific manager.

use super::PackageManager;
use crate::common::distro::OperatingSystem;

/// A specific package definition for a package manager.
///
/// This struct defines how a package should be installed using a particular
/// package manager, with optional distro-specific restrictions.
#[derive(Debug, Clone, Copy)]
pub struct PackageDefinition {
    /// The package name in the respective package manager.
    ///
    /// For Flatpak, this is the app ID (e.g., "org.mozilla.firefox").
    /// For Cargo, this is the crate name.
    pub package_name: &'static str,

    /// Which package manager this definition is for.
    pub manager: PackageManager,

    /// Optional: restrict to specific distros (for native managers).
    ///
    /// If `None`, applies to all distros that use this package manager.
    /// If `Some`, only applies if the current OS is in the list or is based on one in the list.
    pub distros: Option<&'static [OperatingSystem]>,
}

impl PackageDefinition {
    /// Create a new package definition that works on all distros with this manager.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crate::common::package::{PackageDefinition, PackageManager};
    ///
    /// let pkg = PackageDefinition::new("firefox", PackageManager::Pacman);
    /// ```
    pub const fn new(name: &'static str, manager: PackageManager) -> Self {
        Self {
            package_name: name,
            manager,
            distros: None,
        }
    }

    /// Check if this package definition applies to the given OS.
    ///
    /// Returns `true` if:
    /// - `distros` is `None` (universal for this package manager), or
    /// - The OS is in the distros list, or
    /// - The OS is based on a distro in the list
    pub fn applies_to(&self, os: &OperatingSystem) -> bool {
        match self.distros {
            None => true, // Universal - applies to all distros with this manager
            Some(distros) => os.in_any_family(distros),
        }
    }

    /// Check if this package definition can be installed now.
    ///
    /// This checks if the package manager is available on the current system.
    pub fn is_available(&self) -> bool {
        self.manager.is_available()
    }

    /// Get the install hint for this package definition.
    ///
    /// Returns a string like "pacman -S firefox" or "cargo install xcolor".
    pub fn install_hint(&self) -> String {
        match self.manager {
            PackageManager::Pacman => format!("pacman -S {}", self.package_name),
            PackageManager::Apt => format!("apt install {}", self.package_name),
            PackageManager::Dnf => format!("dnf install {}", self.package_name),
            PackageManager::Zypper => format!("zypper install {}", self.package_name),
            PackageManager::Flatpak => format!("flatpak install flathub {}", self.package_name),
            PackageManager::Aur => format!("yay -S {}", self.package_name),
            PackageManager::Cargo => format!("cargo install {}", self.package_name),
            PackageManager::Snap => format!("snap install {}", self.package_name),
            PackageManager::Pkg => format!("pkg install {}", self.package_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let pkg = PackageDefinition::new("firefox", PackageManager::Pacman);
        assert_eq!(pkg.package_name, "firefox");
        assert_eq!(pkg.manager, PackageManager::Pacman);
        assert!(pkg.distros.is_none());
    }

    #[test]
    fn test_applies_to_universal() {
        let pkg = PackageDefinition::new("firefox", PackageManager::Pacman);
        // Universal packages apply to any distro
        assert!(pkg.applies_to(&OperatingSystem::Arch));
        assert!(pkg.applies_to(&OperatingSystem::Ubuntu));
    }

    #[test]
    fn test_install_hint() {
        let pacman = PackageDefinition::new("firefox", PackageManager::Pacman);
        assert_eq!(pacman.install_hint(), "pacman -S firefox");

        let flatpak = PackageDefinition::new("org.mozilla.firefox", PackageManager::Flatpak);
        assert_eq!(
            flatpak.install_hint(),
            "flatpak install flathub org.mozilla.firefox"
        );
    }
}
