use colored::Colorize;

use super::types::{BootMode, GpuKind, SystemInfo};

impl std::fmt::Display for BootMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootMode::UEFI64 => write!(f, "UEFI64"),
            BootMode::UEFI32 => write!(f, "UEFI32"),
            BootMode::BIOS => write!(f, "BIOS"),
        }
    }
}

impl std::fmt::Display for GpuKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuKind::Nvidia => write!(f, "NVIDIA"),
            GpuKind::Amd => write!(f, "AMD"),
            GpuKind::Intel => write!(f, "Intel"),
            GpuKind::Other(name) => write!(f, "{}", name),
        }
    }
}

impl GpuKind {
    pub fn to_colored_string(&self) -> colored::ColoredString {
        match self {
            GpuKind::Nvidia => self.to_string().bright_green(),
            GpuKind::Amd => self.to_string().bright_red(),
            GpuKind::Intel => self.to_string().bright_blue(),
            GpuKind::Other(_) => self.to_string().normal(),
        }
    }

    /// Returns driver packages for this GPU.
    /// For NVIDIA, pass the kernel name to get kernel-specific drivers (nvidia, nvidia-lts, nvidia-dkms).
    pub fn get_driver_packages(&self, kernel: Option<&str>) -> Vec<&'static str> {
        match self {
            GpuKind::Nvidia => {
                let mut packages = Vec::new();
                match kernel.unwrap_or("linux") {
                    "linux" => packages.push("nvidia"),
                    "linux-lts" => packages.push("nvidia-lts"),
                    _ => {
                        // Custom kernels (zen, hardened, etc) need DKMS
                        packages.push("nvidia-dkms");
                        packages.push("dkms");
                    }
                }
                packages.push("nvidia-utils");
                packages.push("nvidia-settings");
                packages
            }
            GpuKind::Amd => vec![
                "vulkan-radeon",
                "lib32-vulkan-radeon",
                "libva-mesa-driver",
                "lib32-libva-mesa-driver",
            ],
            GpuKind::Intel => vec!["vulkan-intel", "lib32-vulkan-intel", "intel-media-driver"],
            GpuKind::Other(_) => vec!["mesa", "lib32-mesa"],
        }
    }
}

impl SystemInfo {
    pub fn detect() -> Self {
        let mut info = SystemInfo {
            internet_connected: crate::common::network::check_internet(),
            ..Default::default()
        };

        // Boot mode check
        if std::path::Path::new("/sys/firmware/efi/fw_platform_size").exists() {
            let content =
                std::fs::read_to_string("/sys/firmware/efi/fw_platform_size").unwrap_or_default();
            if content.trim() == "64" {
                info.boot_mode = BootMode::UEFI64;
            } else if content.trim() == "32" {
                info.boot_mode = BootMode::UEFI32;
            }
        } else if std::path::Path::new("/sys/firmware/efi").exists() {
            // Fallback if fw_platform_size doesn't exist but efi does
            info.boot_mode = BootMode::UEFI64;
        }

        // CPU check
        if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
            info.has_amd_cpu = cpuinfo.contains("AuthenticAMD");
            info.has_intel_cpu = cpuinfo.contains("GenuineIntel");
        }

        // GPU check using /sys/class/drm/ approach
        let mut found_gpus = false;
        if let Ok(drm_entries) = std::fs::read_dir("/sys/class/drm") {
            let mut detected_gpus = std::collections::HashSet::new();

            for entry in drm_entries.flatten() {
                if let Ok(path) = entry.path().join("device").read_link()
                    && let Some(path_str) = path.to_str()
                {
                    let path_lower = path_str.to_lowercase();
                    if path_lower.contains("nvidia") {
                        detected_gpus.insert(GpuKind::Nvidia);
                        found_gpus = true;
                    } else if path_lower.contains("amd") || path_lower.contains("radeon") {
                        detected_gpus.insert(GpuKind::Amd);
                        found_gpus = true;
                    } else if path_lower.contains("intel") {
                        detected_gpus.insert(GpuKind::Intel);
                        found_gpus = true;
                    }
                }
            }

            if found_gpus {
                info.gpus = detected_gpus.into_iter().collect();
            }
        }

        // Fallback to lspci if drm detection didn't find anything
        if !found_gpus && let Ok(lspci) = std::process::Command::new("lspci").output() {
            let output = String::from_utf8_lossy(&lspci.stdout);
            let mut detected_gpus = std::collections::HashSet::new();

            if output.to_lowercase().contains("nvidia") {
                detected_gpus.insert(GpuKind::Nvidia);
            }
            if output.to_lowercase().contains("amd")
                || output.to_lowercase().contains("radeon")
                || output.to_lowercase().contains("advanced micro devices")
            {
                detected_gpus.insert(GpuKind::Amd);
            }
            if output.to_lowercase().contains("intel")
                || output.to_lowercase().contains("integrated graphics")
                || output.to_lowercase().contains("hd graphics")
                || output.to_lowercase().contains("iris")
            {
                detected_gpus.insert(GpuKind::Intel);
            }

            info.gpus = detected_gpus.into_iter().collect();
        }

        // VM check
        if let Ok(virt) = std::process::Command::new("systemd-detect-virt").output()
            && virt.status.success()
        {
            info.vm_type = Some(String::from_utf8_lossy(&virt.stdout).trim().to_string());
        }

        // Architecture check
        info.architecture = std::env::consts::ARCH.to_string();

        // Distro check
        info.distro = crate::common::distro::OperatingSystem::detect().to_string();

        // RAM detection
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2
                        && let Ok(kb) = parts[1].parse::<u64>()
                    {
                        info.total_ram_gb = Some(kb / 1024 / 1024); // Convert KB to GB
                    }
                    break;
                }
            }
        }

        info
    }
}
