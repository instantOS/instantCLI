use colored::Colorize;

use crate::arch::arch_linux_variant::ArchLinuxVariant;

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
            GpuKind::NvidiaOpen => write!(f, "NVIDIA (open kernel modules supported)"),
            GpuKind::Amd => write!(f, "AMD"),
            GpuKind::Intel => write!(f, "Intel"),
            GpuKind::Other(name) => write!(f, "{}", name),
        }
    }
}

impl GpuKind {
    pub fn to_colored_string(&self) -> colored::ColoredString {
        match self {
            GpuKind::Nvidia | GpuKind::NvidiaOpen => self.to_string().bright_green(),
            GpuKind::Amd => self.to_string().bright_red(),
            GpuKind::Intel => self.to_string().bright_blue(),
            GpuKind::Other(_) => self.to_string().normal(),
        }
    }

    fn nvidia_from_pci_name(name: &str) -> Self {
        // hwdata starts NVIDIA device names with the chip codename. NVIDIA's
        // open modules require the GSP introduced in Turing (TU).
        match name.as_bytes() {
            [b'T', b'U', digit, ..]
            | [b'G', b'A' | b'H' | b'B' | b'R', digit, ..]
            | [b'A', b'D', digit, ..]
                if digit.is_ascii_digit() =>
            {
                Self::NvidiaOpen
            }
            _ => Self::Nvidia,
        }
    }
}

fn nvidia_pci_name<'a>(pci_ids: &'a str, device: &str) -> Option<&'a str> {
    pci_ids
        .lines()
        .skip_while(|line| !line.starts_with("10de "))
        .skip(1)
        .take_while(|line| line.is_empty() || line.starts_with(['\t', '#']))
        .filter_map(|line| line.strip_prefix('\t'))
        .filter(|line| !line.starts_with('\t'))
        .filter_map(|line| line.split_once(' '))
        .find_map(|(id, name)| id.eq_ignore_ascii_case(device).then(|| name.trim_start()))
}

/// Read PCI display controllers even if the live kernel has no driver for them.
fn detect_pci_gpus(pci_devices: &std::path::Path, pci_ids: &str, info: &mut SystemInfo) {
    let Ok(entries) = std::fs::read_dir(pci_devices) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(event) = std::fs::read_to_string(entry.path().join("uevent")) else {
            continue;
        };
        let field = |key| event.lines().find_map(|line| line.strip_prefix(key));
        let Some(class) = field("PCI_CLASS=").and_then(|hex| u32::from_str_radix(hex, 16).ok())
        else {
            continue;
        };
        if class >> 16 != 0x03 {
            continue;
        }
        let Some((vendor, device)) = field("PCI_ID=").and_then(|id| id.split_once(':')) else {
            continue;
        };
        let gpu = match vendor {
            "10DE" => {
                GpuKind::nvidia_from_pci_name(nvidia_pci_name(pci_ids, device).unwrap_or_default())
            }
            "1002" => GpuKind::Amd,
            "8086" => GpuKind::Intel,
            _ => continue,
        };
        if !info.gpus.contains(&gpu) {
            info.gpus.push(gpu);
        }
    }
    info.gpus.sort_by_key(|gpu| gpu.to_string());
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

/// Map a DMI product or vendor string to the name `systemd-detect-virt` would
/// report for the same hypervisor.
///
/// This is a fallback for hosts without `systemd-detect-virt` (Artix and other
/// non-systemd systems), so it only needs the common virtual-machine strings.
fn vm_type_from_dmi(product_name: &str) -> Option<&'static str> {
    let product = product_name.trim().to_ascii_lowercase();
    if product.is_empty() {
        return None;
    }

    if product.contains("kvm") {
        Some("kvm")
    } else if product.contains("qemu") || product.contains("bochs") {
        Some("qemu")
    } else if product.contains("vmware") {
        Some("vmware")
    } else if product.contains("virtualbox") || product.contains("innotek") {
        Some("oracle")
    } else {
        None
    }
}

/// Read the DMI product name, then the vendor, as a VM-detection fallback.
fn detect_vm_type_from_dmi() -> Option<String> {
    for path in [
        "/sys/class/dmi/id/product_name",
        "/sys/class/dmi/id/sys_vendor",
    ] {
        let Ok(value) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(vm_type) = vm_type_from_dmi(&value) {
            return Some(vm_type.to_string());
        }
    }
    None
}

impl SystemInfo {
    pub(crate) fn nvidia_fallback_notice(
        &self,
        variant: Option<ArchLinuxVariant>,
    ) -> Option<String> {
        if !self.gpus.contains(&GpuKind::Nvidia) {
            return None;
        }

        let mut notice = String::from(
            "Using Nouveau for NVIDIA graphics because nvidia-open support could not be confirmed for every NVIDIA GPU. Graphics performance may be limited.",
        );
        if variant == Some(ArchLinuxVariant::Arch) {
            notice.push_str(" After installation, GTX 10-series and supported Maxwell cards can use nvidia-580xx-dkms from the AUR with headers for the installed kernel.");
        }
        Some(notice)
    }

    pub fn detect() -> Self {
        Self::detect_with_network(crate::common::network::check_internet())
    }

    /// Install flows supply their HTTPS probe; hardware detection stays
    /// synchronous and does not perform a second, conflicting ICMP probe.
    pub fn detect_with_network(internet_connected: bool) -> Self {
        let mut info = SystemInfo {
            internet_connected,
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

        let pci_ids = std::fs::read_to_string("/usr/share/hwdata/pci.ids").unwrap_or_default();
        detect_pci_gpus(
            std::path::Path::new("/sys/bus/pci/devices"),
            &pci_ids,
            &mut info,
        );

        // VM check. `systemd-detect-virt` is authoritative when it exists; on
        // non-systemd hosts (Artix) fall back to the DMI product name.
        info.vm_type = std::process::Command::new("systemd-detect-virt")
            .output()
            .ok()
            .filter(|virt| virt.status.success())
            .map(|virt| String::from_utf8_lossy(&virt.stdout).trim().to_string())
            .or_else(detect_vm_type_from_dmi);

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

#[cfg(test)]
mod tests {
    use super::{GpuKind, SystemInfo, detect_pci_gpus, vm_type_from_dmi};

    #[test]
    fn pci_detection_distinguishes_pascal_and_turing_without_counting_audio() -> anyhow::Result<()>
    {
        const PCI_IDS: &str = concat!(
            "10de  NVIDIA Corporation\n",
            "\t1c02  GP106 [GeForce GTX 1060 3GB]\n",
            "\t1c81  GP107 [GeForce GTX 1050]\n",
            "\t1f82  TU117 [GeForce GTX 1650]\n",
            "\t10fa  TU117 High Definition Audio Controller\n",
        );
        let pci = tempfile::tempdir()?;
        for (slot, event) in [
            ("00:01.0", "PCI_CLASS=30000\nPCI_ID=8086:9A49\n"),
            ("01:00.0", "PCI_CLASS=30200\nPCI_ID=10DE:1F82\n"),
            ("01:00.1", "PCI_CLASS=40300\nPCI_ID=10DE:10FA\n"),
        ] {
            let path = pci.path().join(slot);
            std::fs::create_dir(&path)?;
            std::fs::write(path.join("uevent"), event)?;
        }

        let mut info = SystemInfo::default();
        detect_pci_gpus(pci.path(), PCI_IDS, &mut info);
        assert_eq!(info.gpus, vec![GpuKind::Intel, GpuKind::NvidiaOpen]);

        let mut info = SystemInfo::default();
        detect_pci_gpus(pci.path(), "", &mut info);
        assert_eq!(info.gpus, vec![GpuKind::Intel, GpuKind::Nvidia]);

        let legacy = pci.path().join("02:00.0");
        std::fs::create_dir(&legacy)?;
        for id in ["1C81", "1C02"] {
            std::fs::write(
                legacy.join("uevent"),
                format!("PCI_CLASS=30000\nPCI_ID=10DE:{id}\n"),
            )?;
            let mut info = SystemInfo::default();
            detect_pci_gpus(pci.path(), PCI_IDS, &mut info);
            assert_eq!(
                info.gpus,
                vec![GpuKind::Intel, GpuKind::Nvidia, GpuKind::NvidiaOpen]
            );
        }
        Ok(())
    }

    #[test]
    fn dmi_product_names_map_to_vm_types() {
        for (product, expected) in [
            ("KVM", "kvm"),
            ("kvm", "kvm"),
            ("QEMU", "qemu"),
            ("Bochs", "qemu"),
            ("VMware Virtual Platform", "vmware"),
            ("VirtualBox", "oracle"),
            ("innotek GmbH", "oracle"),
        ] {
            assert_eq!(vm_type_from_dmi(product), Some(expected), "{product}");
        }
    }

    #[test]
    fn unrecognized_dmi_does_not_identify_a_hypervisor() {
        assert_eq!(vm_type_from_dmi(""), None);
        assert_eq!(vm_type_from_dmi("   "), None);
        assert_eq!(vm_type_from_dmi("X570 AORUS ELITE"), None);
    }
}
