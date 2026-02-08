use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Represents a detected operating system with methods for family checks
/// and package manager detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatingSystem {
    /// instantOS (Arch-based)
    InstantOS,
    /// Vanilla Arch Linux
    Arch,
    /// Manjaro Linux
    Manjaro,
    /// EndeavourOS
    EndeavourOS,
    /// SteamOS (immutable Arch-based)
    SteamOS,
    /// Bazzite (immutable Fedora-based gaming distro)
    Bazzite,
    /// Debian
    Debian,
    /// Ubuntu
    Ubuntu,
    /// Pop!_OS (Ubuntu-based)
    PopOS,
    /// Linux Mint (Ubuntu/Debian-based)
    LinuxMint,
    /// Fedora
    Fedora,
    /// CentOS
    CentOS,
    /// OpenSUSE (including Leap and Tumbleweed)
    OpenSUSE,
    /// Termux (Android)
    Termux,
    /// Unknown distribution with ID
    Unknown(String),
}

impl OperatingSystem {
    /// Detect the current operating system from /etc/os-release
    pub fn detect() -> Self {
        let os_release_path = Path::new("/etc/os-release");
        if os_release_path.exists() {
            return match fs::read_to_string(os_release_path) {
                Ok(content) => Self::parse_os_release(&content),
                Err(_) => Self::Unknown("Failed to read /etc/os-release".to_string()),
            };
        }

        // Check for Termux environment if /etc/os-release not found
        if std::env::var("TERMUX_VERSION").is_ok() {
            return Self::Termux;
        }

        Self::Unknown("No /etc/os-release found".to_string())
    }

    /// Parse os-release content and return the detected OS
    fn parse_os_release(content: &str) -> Self {
        let mut id = String::new();
        let mut id_like = String::new();

        for line in content.lines() {
            if let Some(val) = line.strip_prefix("ID=") {
                id = val.trim_matches('"').to_string();
            } else if let Some(val) = line.strip_prefix("ID_LIKE=") {
                id_like = val.trim_matches('"').to_string();
            }
        }

        match id.as_str() {
            "instantos" => Self::InstantOS,
            "arch" => Self::Arch,
            "manjaro" => Self::Manjaro,
            "endeavouros" => Self::EndeavourOS,
            "steamos" => Self::SteamOS,
            "bazzite" => Self::Bazzite,
            "debian" => Self::Debian,
            "ubuntu" => Self::Ubuntu,
            "pop" => Self::PopOS,
            "linuxmint" => Self::LinuxMint,
            "fedora" => Self::Fedora,
            "centos" => Self::CentOS,
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" => Self::OpenSUSE,
            _ => {
                // For unknown IDs, check ID_LIKE for family detection
                if id_like.contains("arch") {
                    Self::Arch
                } else if id_like.contains("ubuntu") {
                    Self::Ubuntu
                } else if id_like.contains("debian") {
                    Self::Debian
                } else if id_like.contains("fedora") || id_like.contains("rhel") {
                    Self::Fedora
                } else {
                    Self::Unknown(id)
                }
            }
        }
    }

    // ========================================================================
    // Parent/Base OS
    // ========================================================================

    /// Returns the parent OS that this distribution is based on.
    /// Returns `None` for root distributions (Arch, Debian, Fedora, etc.)
    /// and for Unknown.
    pub fn based_on(&self) -> Option<Self> {
        match self {
            // Arch-based
            Self::InstantOS | Self::Manjaro | Self::EndeavourOS | Self::SteamOS => Some(Self::Arch),
            // Fedora-based
            Self::Bazzite => Some(Self::Fedora),
            // Ubuntu-based (Ubuntu itself is Debian-based)
            Self::PopOS | Self::LinuxMint => Some(Self::Ubuntu),
            Self::Ubuntu => Some(Self::Debian),
            // Termux is Debian-based (uses apt/dpkg structure)
            Self::Termux => Some(Self::Debian),
            // CentOS is Fedora/RHEL-based
            Self::CentOS => Some(Self::Fedora),
            // Root distributions and Unknown have no parent
            Self::Arch | Self::Debian | Self::Fedora | Self::OpenSUSE | Self::Unknown(_) => None,
        }
    }

    /// Check if this OS belongs to the target OS family.
    ///
    /// Returns true if self is the same as target, or if self is transitively
    /// based on target (i.e., target is somewhere in self's ancestry chain).
    ///
    /// # Examples
    /// ```
    /// use crate::common::distro::OperatingSystem;
    ///
    /// // Exact match - Debian is in the Debian family
    /// assert!(OperatingSystem::Debian.in_family(&OperatingSystem::Debian));
    ///
    /// // Direct derivative - Ubuntu is based on Debian
    /// assert!(OperatingSystem::Ubuntu.in_family(&OperatingSystem::Debian));
    ///
    /// // Transitive derivative - PopOS -> Ubuntu -> Debian
    /// assert!(OperatingSystem::PopOS.in_family(&OperatingSystem::Debian));
    ///
    /// // Not related - Fedora is not in the Debian family
    /// assert!(!OperatingSystem::Fedora.in_family(&OperatingSystem::Debian));
    /// ```
    pub fn in_family(&self, root: &OperatingSystem) -> bool {
        self == root || self.based_on().map_or(false, |p| p.in_family(root))
    }

    /// Check if this OS belongs to any of the target families.
    ///
    /// This is useful for checking compatibility with multiple supported families.
    ///
    /// # Examples
    /// ```
    /// use crate::common::distro::OperatingSystem;
    ///
    /// // Check if OS is Arch-family or Debian-family
    /// let supported = &[OperatingSystem::Arch, OperatingSystem::Debian];
    /// assert!(OperatingSystem::Manjaro.in_any_family(supported));
    /// assert!(OperatingSystem::Ubuntu.in_any_family(supported));
    /// assert!(!OperatingSystem::Fedora.in_any_family(supported));
    /// ```
    pub fn in_any_family(&self, roots: &[OperatingSystem]) -> bool {
        roots.iter().any(|r| self.in_family(r))
    }

    /// Check if this OS is immutable (read-only root filesystem)
    /// Immutable OSes cannot be modified in the traditional way and
    /// updates replace the entire OS image.
    pub fn is_immutable(&self) -> bool {
        matches!(self, Self::SteamOS | Self::Bazzite)
    }

    // ========================================================================
    // Package Manager Integration
    // ========================================================================

    /// Get the native package manager for this operating system.
    ///
    /// This is the single source of truth for the OS → native package manager mapping.
    /// Returns the new unified `PackageManager` enum from `crate::common::package`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crate::common::distro::OperatingSystem;
    /// use crate::common::package::PackageManager;
    ///
    /// let os = OperatingSystem::detect();
    /// match os.native_package_manager() {
    ///     Some(PackageManager::Pacman) => println!("Arch-based!"),
    ///     Some(PackageManager::Apt) => println!("Debian-based!"),
    ///     _ => println!("Other or unknown"),
    /// }
    /// ```
    pub fn native_package_manager(&self) -> Option<crate::common::package::PackageManager> {
        use crate::common::package::PackageManager as PM;
        match self {
            // Root distributions
            Self::Arch => Some(PM::Pacman),
            Self::Debian => Some(PM::Apt),
            Self::Fedora => Some(PM::Dnf),
            Self::CentOS => Some(PM::Dnf),
            Self::OpenSUSE => Some(PM::Zypper),
            Self::Termux => Some(PM::Pkg),

            // Unknown has no supported manager
            Self::Unknown(_) => None,

            // All derivatives fall back to parent
            _ => self.based_on().and_then(|p| p.native_package_manager()),
        }
    }

    /// Get the display name of the operating system
    pub fn name(&self) -> &str {
        match self {
            Self::InstantOS => "instantOS",
            Self::Arch => "Arch Linux",
            Self::Manjaro => "Manjaro",
            Self::EndeavourOS => "EndeavourOS",
            Self::SteamOS => "SteamOS",
            Self::Bazzite => "Bazzite",
            Self::Debian => "Debian",
            Self::Ubuntu => "Ubuntu",
            Self::PopOS => "Pop!_OS",
            Self::LinuxMint => "Linux Mint",
            Self::Fedora => "Fedora",
            Self::CentOS => "CentOS",
            Self::OpenSUSE => "openSUSE",
            Self::Termux => "Termux",
            Self::Unknown(_) => "Unknown",
        }
    }
}

impl std::fmt::Display for OperatingSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(name) => write!(f, "Unknown ({})", name),
            _ => write!(f, "{}", self.name()),
        }
    }
}

/// Check if running from a live ISO
pub fn is_live_iso() -> bool {
    Path::new("/run/archiso/cowspace").exists()
}

/// Increase the cowspace size on live ISO
pub fn increase_cowspace() -> Result<()> {
    if !is_live_iso() {
        return Ok(());
    }

    let total_ram_mb = get_total_ram_mb().unwrap_or(4096); // Default to 4GB if detection fails

    // Calculate 70% of RAM
    let target_size_mb = (total_ram_mb as f64 * 0.7) as u64;

    // Cap at 10GB (10 * 1024 MB)
    let max_size_mb = 10 * 1024;
    let size_mb = std::cmp::min(target_size_mb, max_size_mb);

    let size_str = format!("{}M", size_mb);

    println!(
        "Increasing cowspace to {} (Total RAM: {}MB)...",
        size_str, total_ram_mb
    );

    let status = std::process::Command::new("mount")
        .arg("-o")
        .arg(format!("remount,size={}", size_str))
        .arg("/run/archiso/cowspace")
        .status()
        .context("Failed to execute mount command")?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to remount cowspace"));
    }

    Ok(())
}

fn get_total_ram_mb() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            // Format: MemTotal:        16303832 kB
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2
                && let Ok(kb) = parts[1].parse::<u64>()
            {
                return Some(kb / 1024);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_arch() {
        let content = r#"NAME="Arch Linux"
PRETTY_NAME="Arch Linux"
ID=arch
BUILD_ID=rolling
ANSI_COLOR="38;2;23;147;209"
HOME_URL="https://archlinux.org/"
DOCUMENTATION_URL="https://wiki.archlinux.org/"
SUPPORT_URL="https://bbs.archlinux.org/"
BUG_REPORT_URL="https://bugs.archlinux.org/"
LOGO=archlinux-logo"#;
        assert_eq!(
            OperatingSystem::parse_os_release(content),
            OperatingSystem::Arch
        );
    }

    #[test]
    fn test_parse_ubuntu() {
        let content = r#"PRETTY_NAME="Ubuntu 22.04.3 LTS"
NAME="Ubuntu"
VERSION_ID="22.04"
VERSION="22.04.3 LTS (Jammy Jellyfish)"
VERSION_CODENAME=jammy
ID=ubuntu
ID_LIKE=debian
HOME_URL="https://www.ubuntu.com/"
SUPPORT_URL="https://help.ubuntu.com/"
BUG_REPORT_URL="https://bugs.launchpad.net/ubuntu/"
PRIVACY_POLICY_URL="https://www.ubuntu.com/legal/terms-and-policies/privacy-policy"
UBUNTU_CODENAME=jammy"#;
        assert_eq!(
            OperatingSystem::parse_os_release(content),
            OperatingSystem::Ubuntu
        );
    }

    #[test]
    fn test_parse_instantos() {
        let content = r#"NAME="instantOS"
PRETTY_NAME="instantOS"
ID="instantos"
BUILD_ID=rolling
ANSI_COLOR="38;2;23;147;209"
HOME_URL="https://archlinux.org/"
DOCUMENTATION_URL="https://wiki.archlinux.org/"
SUPPORT_URL="https://bbs.archlinux.org/"
BUG_REPORT_URL="https://gitlab.archlinux.org/groups/archlinux/-/issues"
PRIVACY_POLICY_URL="https://terms.archlinux.org/docs/privacy-policy/"
LOGO=archlinux-logo
ID_LIKE="arch""#;
        let os = OperatingSystem::parse_os_release(content);
        assert_eq!(os, OperatingSystem::InstantOS);
        assert!(os.in_family(&OperatingSystem::Arch));
    }

    #[test]
    fn test_parse_unknown_arch_based() {
        let content = r#"NAME="Custom Arch"
PRETTY_NAME="Custom Arch Distro"
ID="customarch"
ID_LIKE="arch""#;
        let os = OperatingSystem::parse_os_release(content);
        // Falls back to Arch for unknown arch-based distros
        assert_eq!(os, OperatingSystem::Arch);
        assert!(os.in_family(&OperatingSystem::Arch));
    }

    #[test]
    fn test_family_checks() {
        // Arch family - use in_family for all family checks
        assert!(OperatingSystem::Arch.in_family(&OperatingSystem::Arch));
        assert!(OperatingSystem::InstantOS.in_family(&OperatingSystem::Arch));
        assert!(OperatingSystem::Manjaro.in_family(&OperatingSystem::Arch));
        assert!(OperatingSystem::EndeavourOS.in_family(&OperatingSystem::Arch));
        assert!(!OperatingSystem::Ubuntu.in_family(&OperatingSystem::Arch));
        assert!(!OperatingSystem::Fedora.in_family(&OperatingSystem::Arch));
        assert!(!OperatingSystem::Termux.in_family(&OperatingSystem::Arch));

        // Debian family (includes Ubuntu and derivatives)
        assert!(OperatingSystem::Debian.in_family(&OperatingSystem::Debian));
        assert!(OperatingSystem::Ubuntu.in_family(&OperatingSystem::Debian));
        assert!(OperatingSystem::PopOS.in_family(&OperatingSystem::Debian));
        assert!(OperatingSystem::LinuxMint.in_family(&OperatingSystem::Debian));
        assert!(OperatingSystem::Termux.in_family(&OperatingSystem::Debian));

        // Fedora family
        assert!(OperatingSystem::Fedora.in_family(&OperatingSystem::Fedora));
        assert!(OperatingSystem::CentOS.in_family(&OperatingSystem::Fedora));
        assert!(OperatingSystem::Bazzite.in_family(&OperatingSystem::Fedora));
    }

    #[test]
    fn test_package_manager() {
        use crate::common::package::PackageManager;

        assert_eq!(
            OperatingSystem::Arch.native_package_manager(),
            Some(PackageManager::Pacman)
        );
        assert_eq!(
            OperatingSystem::InstantOS.native_package_manager(),
            Some(PackageManager::Pacman)
        );
        assert_eq!(
            OperatingSystem::Ubuntu.native_package_manager(),
            Some(PackageManager::Apt)
        );
        assert_eq!(
            OperatingSystem::Debian.native_package_manager(),
            Some(PackageManager::Apt)
        );
        assert_eq!(
            OperatingSystem::Fedora.native_package_manager(),
            Some(PackageManager::Dnf)
        );
        assert_eq!(
            OperatingSystem::Termux.native_package_manager(),
            Some(PackageManager::Pkg)
        );
    }

    #[test]
    fn test_based_on() {
        // Arch derivatives
        assert_eq!(
            OperatingSystem::InstantOS.based_on(),
            Some(OperatingSystem::Arch)
        );
        assert_eq!(
            OperatingSystem::Manjaro.based_on(),
            Some(OperatingSystem::Arch)
        );
        assert_eq!(
            OperatingSystem::EndeavourOS.based_on(),
            Some(OperatingSystem::Arch)
        );

        // Ubuntu/Debian chain
        assert_eq!(
            OperatingSystem::PopOS.based_on(),
            Some(OperatingSystem::Ubuntu)
        );
        assert_eq!(
            OperatingSystem::LinuxMint.based_on(),
            Some(OperatingSystem::Ubuntu)
        );
        assert_eq!(
            OperatingSystem::Ubuntu.based_on(),
            Some(OperatingSystem::Debian)
        );

        // Termux
        assert_eq!(
            OperatingSystem::Termux.based_on(),
            Some(OperatingSystem::Debian)
        );

        // Root distributions have no parent
        assert_eq!(OperatingSystem::Arch.based_on(), None);
        assert_eq!(OperatingSystem::Debian.based_on(), None);
        assert_eq!(OperatingSystem::Fedora.based_on(), None);
    }

    #[test]
    fn test_in_any_family() {
        // Test checking membership in multiple families
        let arch_or_debian = &[OperatingSystem::Arch, OperatingSystem::Debian];

        // Arch derivatives match
        assert!(OperatingSystem::Arch.in_any_family(arch_or_debian));
        assert!(OperatingSystem::InstantOS.in_any_family(arch_or_debian));
        assert!(OperatingSystem::Manjaro.in_any_family(arch_or_debian));

        // Debian derivatives match
        assert!(OperatingSystem::Debian.in_any_family(arch_or_debian));
        assert!(OperatingSystem::Ubuntu.in_any_family(arch_or_debian));
        assert!(OperatingSystem::PopOS.in_any_family(arch_or_debian));

        // Fedora does not match
        assert!(!OperatingSystem::Fedora.in_any_family(arch_or_debian));

        // Single family checks using in_family
        assert!(OperatingSystem::Ubuntu.in_family(&OperatingSystem::Debian));
        assert!(!OperatingSystem::Debian.in_family(&OperatingSystem::Ubuntu)); // Parent not child
        assert!(OperatingSystem::InstantOS.in_family(&OperatingSystem::Arch));
        assert!(!OperatingSystem::Arch.in_family(&OperatingSystem::InstantOS)); // Parent not child
    }

    #[test]
    fn test_parse_steamos() {
        let content = r#"NAME="SteamOS"
PRETTY_NAME="SteamOS 3.6.19"
ID=steamos
ID_LIKE=arch
VARIANT_ID=steamdeck
VERSION_ID=3.6.19
BUILD_ID=20250313.1
STEAMOS_DEFAULT_UPDATE_BRANCH=stable"#;
        let os = OperatingSystem::parse_os_release(content);
        assert_eq!(os, OperatingSystem::SteamOS);
        assert!(os.in_family(&OperatingSystem::Arch));
        assert!(os.is_immutable());
        assert_eq!(os.name(), "SteamOS");
    }

    #[test]
    fn test_is_immutable() {
        // SteamOS is immutable
        assert!(OperatingSystem::SteamOS.is_immutable());

        // Other Arch-based distros are not immutable
        assert!(!OperatingSystem::Arch.is_immutable());
        assert!(!OperatingSystem::InstantOS.is_immutable());
        assert!(!OperatingSystem::Manjaro.is_immutable());
        assert!(!OperatingSystem::EndeavourOS.is_immutable());

        // Other distros are not immutable
        assert!(!OperatingSystem::Debian.is_immutable());
        assert!(!OperatingSystem::Ubuntu.is_immutable());
        assert!(!OperatingSystem::Fedora.is_immutable());
    }

    #[test]
    fn test_steamos_based_on_arch() {
        assert_eq!(
            OperatingSystem::SteamOS.based_on(),
            Some(OperatingSystem::Arch)
        );
    }

    #[test]
    fn test_parse_bazzite() {
        let content = r#"NAME="Bazzite"
VERSION="43.20251210.0 (Kinoite)"
ID=bazzite
ID_LIKE="fedora"
VERSION_ID=43
PRETTY_NAME="Bazzite"
ANSI_COLOR="0;38;2;138;43;226"
LOGO=bazzite-logo-icon"#;
        let os = OperatingSystem::parse_os_release(content);
        assert_eq!(os, OperatingSystem::Bazzite);
        assert_eq!(os.name(), "Bazzite");
    }

    #[test]
    fn test_bazzite_is_fedora_based() {
        assert_eq!(
            OperatingSystem::Bazzite.based_on(),
            Some(OperatingSystem::Fedora)
        );
    }

    #[test]
    fn test_bazzite_is_immutable() {
        assert!(OperatingSystem::Bazzite.is_immutable());
    }

    #[test]
    fn test_bazzite_is_not_arch_family() {
        assert!(!OperatingSystem::Bazzite.in_family(&OperatingSystem::Arch));
    }

    #[test]
    fn test_bazzite_package_manager() {
        use crate::common::package::PackageManager;
        assert_eq!(
            OperatingSystem::Bazzite.native_package_manager(),
            Some(PackageManager::Dnf)
        );
    }
}
