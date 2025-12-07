use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Distro {
    Arch,
    Debian,
    Ubuntu,
    Fedora,
    CentOS,
    OpenSUSE,
    Manjaro,
    EndeavourOS,
    PopOS,
    LinuxMint,
    Unknown(String),
}

impl std::fmt::Display for Distro {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Distro::Arch => {
                // Check if we're running on instantOS specifically
                if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
                    for line in content.lines() {
                        if let Some(val) = line.strip_prefix("ID=")
                            && val.trim_matches('"') == "instantos"
                        {
                            return write!(f, "instantOS (Arch-based)");
                        }
                    }
                }
                write!(f, "Arch Linux")
            }
            Distro::Debian => write!(f, "Debian"),
            Distro::Ubuntu => write!(f, "Ubuntu"),
            Distro::Fedora => write!(f, "Fedora"),
            Distro::CentOS => write!(f, "CentOS"),
            Distro::OpenSUSE => write!(f, "OpenSUSE"),
            Distro::Manjaro => write!(f, "Manjaro"),
            Distro::EndeavourOS => write!(f, "EndeavourOS"),
            Distro::PopOS => write!(f, "Pop!_OS"),
            Distro::LinuxMint => write!(f, "Linux Mint"),
            Distro::Unknown(name) => write!(f, "Unknown ({})", name),
        }
    }
}

pub fn detect_distro() -> Result<Distro> {
    let os_release_path = Path::new("/etc/os-release");
    if !os_release_path.exists() {
        return Ok(Distro::Unknown("No /etc/os-release found".to_string()));
    }

    let content = fs::read_to_string(os_release_path).context("Failed to read /etc/os-release")?;

    parse_os_release(&content)
}

/// Check if the system is running instantOS
pub fn is_instantos() -> bool {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("ID=") {
                return val.trim_matches('"') == "instantos";
            }
        }
    }
    false
}

pub fn is_live_iso() -> bool {
    Path::new("/run/archiso/cowspace").exists()
}

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

fn parse_os_release(content: &str) -> Result<Distro> {
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
        "arch" => Ok(Distro::Arch),
        "debian" => Ok(Distro::Debian),
        "ubuntu" => Ok(Distro::Ubuntu),
        "fedora" => Ok(Distro::Fedora),
        "centos" => Ok(Distro::CentOS),
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" => Ok(Distro::OpenSUSE),
        "manjaro" => Ok(Distro::Manjaro),
        "endeavouros" => Ok(Distro::EndeavourOS),
        "pop" => Ok(Distro::PopOS),
        "linuxmint" => Ok(Distro::LinuxMint),
        "instantos" => {
            // instantOS is Arch-based, check ID_LIKE to confirm
            if id_like.contains("arch") {
                Ok(Distro::Arch)
            } else {
                Ok(Distro::Unknown(id))
            }
        }
        _ => {
            // For unknown IDs, check if they are Arch-based
            if id_like.contains("arch") {
                Ok(Distro::Arch)
            } else {
                Ok(Distro::Unknown(id))
            }
        }
    }
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
        assert_eq!(parse_os_release(content).unwrap(), Distro::Arch);
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
        assert_eq!(parse_os_release(content).unwrap(), Distro::Ubuntu);
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
        assert_eq!(parse_os_release(content).unwrap(), Distro::Arch);
    }

    #[test]
    fn test_parse_unknown_arch_based() {
        let content = r#"NAME="Custom Arch"
PRETTY_NAME="Custom Arch Distro"
ID="customarch"
ID_LIKE="arch""#;
        assert_eq!(parse_os_release(content).unwrap(), Distro::Arch);
    }
}
