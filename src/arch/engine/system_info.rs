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
    /// For NVIDIA, the kernel determines the driver (nvidia, nvidia-lts, or dkms).
    pub fn get_driver_packages(
        &self,
        kernel: Option<crate::arch::engine::Kernel>,
    ) -> Vec<&'static str> {
        match self {
            GpuKind::Nvidia => {
                let mut packages = Vec::new();
                match kernel.unwrap_or(crate::arch::engine::Kernel::Linux) {
                    crate::arch::engine::Kernel::Linux => packages.push("nvidia"),
                    crate::arch::engine::Kernel::Lts => packages.push("nvidia-lts"),
                    crate::arch::engine::Kernel::Zen => {
                        // Zen needs DKMS instead of a prebuilt module
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

    /// Kernel headers required to build this GPU's driver on the given
    /// kernel, if any. DKMS drivers (e.g. nvidia-dkms on Zen) need the
    /// headers to compile the out-of-tree module; prebuilt drivers do not.
    /// Sibling of [`Self::get_driver_packages`] so the DKMS/headers coupling
    /// stays in one place.
    pub fn get_kernel_headers(&self, kernel: crate::arch::engine::Kernel) -> Option<String> {
        match (self, kernel) {
            (GpuKind::Nvidia, crate::arch::engine::Kernel::Zen) => {
                Some(format!("{}-headers", kernel.label()))
            }
            _ => None,
        }
    }
}

/// PCI/USB vendor IDs of every network interface, read from sysfs.
///
/// Used to select the `linux-firmware-*` split packages that actually match
/// the machine's hardware (see `execution::base::firmware_packages`). USB
/// devices keep their `vendor` file on the USB device directory above the
/// interface, so the lookup walks up a few sysfs levels.
pub fn detect_network_vendor_ids() -> Vec<String> {
    let mut vendors = std::collections::HashSet::new();

    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let Ok(device) = entry.path().join("device").read_link() else {
            continue;
        };

        // Walk up the sysfs device tree until we find a `vendor` file.
        // PCI devices expose it at the function (one hop up); USB and other
        // buses nest deeper; the natural top of the device hierarchy is
        // `/sys/devices`, so stop once we climb above it.
        let mut current = device.as_path();
        loop {
            match std::fs::read_to_string(current.join("vendor")) {
                Ok(vendor) => {
                    vendors.insert(vendor.trim().trim_start_matches("0x").to_ascii_lowercase());
                    break;
                }
                Err(_) => match current.parent() {
                    Some(parent) if parent.starts_with("/sys/devices") => current = parent,
                    _ => break,
                },
            }
        }
    }

    vendors.into_iter().collect()
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

        // Network interface vendors, used for firmware split selection
        info.network_vendor_ids = detect_network_vendor_ids();

        // Bluetooth adapter presence: the live environment's kernel binds
        // btusb automatically, so a bound hci device means real hardware.
        info.has_bluetooth = std::fs::read_dir("/sys/class/bluetooth")
            .map(|entries| {
                entries
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().starts_with("hci"))
            })
            .unwrap_or(false);

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

    pub fn print_system_info(&self) {
        use crate::common::compositor::CompositorType;
        use crate::ui::nerd_font::NerdFont;

        let print_row =
            |icon: colored::ColoredString, label: &str, value: &dyn std::fmt::Display| {
                println!("  {}   {:<20} {}", icon, label, value);
            };

        println!();
        println!(
            "  {} {}",
            NerdFont::Desktop.to_string().bright_cyan(),
            "System Information".bright_white().bold()
        );
        println!("  {}", "─".repeat(50).bright_black());

        print_row(
            NerdFont::Terminal.to_string().bright_cyan(),
            "Distro:",
            &self.distro.bright_cyan(),
        );

        let boot_mode_str = match self.boot_mode {
            super::types::BootMode::UEFI64 => "UEFI 64-bit",
            super::types::BootMode::UEFI32 => "UEFI 32-bit",
            super::types::BootMode::BIOS => "BIOS",
        };
        print_row(
            NerdFont::PowerOff.to_string().bright_green(),
            "Boot Mode:",
            &boot_mode_str.bright_green(),
        );

        print_row(
            NerdFont::Cpu.to_string().bright_magenta(),
            "Architecture:",
            &self.architecture.bright_magenta(),
        );

        if let Some(ram_gb) = self.total_ram_gb {
            let ram_str = format!("{} GB", ram_gb);
            let colored = if ram_gb >= 4 {
                ram_str.bright_green()
            } else if ram_gb >= 1 {
                ram_str.bright_yellow()
            } else {
                ram_str.bright_red()
            };
            print_row(
                NerdFont::Memory.to_string().bright_cyan(),
                "Memory:",
                &colored,
            );
        }

        if self.has_intel_cpu {
            print_row(
                NerdFont::Cpu.to_string().bright_blue(),
                "CPU:",
                &"Intel".bright_blue(),
            );
        } else if self.has_amd_cpu {
            print_row(
                NerdFont::Cpu.to_string().bright_red(),
                "CPU:",
                &"AMD".bright_red(),
            );
        }

        if !self.gpus.is_empty() {
            let colored_gpu_str = if self.gpus.len() == 1 {
                self.gpus[0].to_colored_string()
            } else {
                let gpu_strs: Vec<String> = self.gpus.iter().map(|gpu| gpu.to_string()).collect();
                gpu_strs.join(", ").normal()
            };

            print_row(
                NerdFont::Gpu.to_string().bright_cyan(),
                "GPU:",
                &colored_gpu_str,
            );
        }

        if let Some(vm_type) = &self.vm_type {
            println!(
                "  {}   {:<20} {} ({})",
                NerdFont::Server.to_string().bright_yellow(),
                "Virtualization:",
                vm_type.bright_yellow(),
                "Virtual Machine".bright_black()
            );
        } else {
            print_row(
                NerdFont::Server.to_string().bright_green(),
                "Virtualization:",
                &"Bare Metal".bright_green(),
            );
        }

        if self.internet_connected {
            print_row(
                NerdFont::Globe.to_string().bright_green(),
                "Internet:",
                &"Connected".bright_green(),
            );
        } else {
            print_row(
                NerdFont::Globe.to_string().bright_red(),
                "Internet:",
                &"Disconnected".bright_red(),
            );
        }

        let compositor = CompositorType::detect();
        let compositor_str = format!("{} ({})", compositor.name(), compositor.display_server());
        print_row(
            NerdFont::Monitor.to_string().bright_yellow(),
            "Compositor:",
            &compositor_str.bright_yellow(),
        );

        println!("  {}", "─".repeat(50).bright_black());
        println!();
    }
}
