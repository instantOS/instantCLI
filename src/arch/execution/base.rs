use super::CommandRunner;
use crate::arch::engine::{GpuKind, InstallPlan};
use anyhow::{Context, Result};
use std::collections::HashSet;

use super::packages::strings;

pub async fn install_base(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Setting up mirrors...");
    setup_mirrors(plan, executor).await?;

    println!("Configuring pacman settings...");
    crate::common::pacman::configure_pacman_settings(None, executor.dry_run()).await?;

    println!("Installing base system...");
    run_pacstrap(plan, executor)?;

    Ok(())
}

async fn setup_mirrors(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    // Check if a region was selected (question may have been skipped if fetch failed)
    let region_name = plan.mirror_region.as_ref();

    if executor.dry_run() {
        match region_name {
            Some(region) => {
                println!("[DRY RUN] Fetching mirrorlist for region: {}", region);
            }
            None => {
                println!("[DRY RUN] Using fallback mirrorlist (region selection was skipped)");
            }
        }
        println!("[DRY RUN] Writing to /etc/pacman.d/mirrorlist");
        return Ok(());
    }

    let mirrorlist = match region_name {
        Some(region) => {
            // Normal path: user selected a region
            println!("Selected region: {}", region);
            fetch_mirrorlist_for_region(region).await?
        }
        None => {
            // Fallback path: region selection was skipped (fetch failed)
            println!("Mirror region selection was skipped, using fallback mirrorlist...");
            fetch_fallback_mirrorlist().await?
        }
    };

    // Write to file
    std::fs::write("/etc/pacman.d/mirrorlist", mirrorlist)?;
    println!("Mirrors updated.");

    Ok(())
}

/// Fetch mirrorlist for a specific region
async fn fetch_mirrorlist_for_region(region_name: &str) -> Result<String> {
    // Fetch region map to get code
    let regions = crate::arch::mirrors::fetch_mirror_regions().await?;
    let region_code = regions
        .get(region_name)
        .context(format!("Could not find code for region: {}", region_name))?;

    println!("Fetching mirrors for code: {}", region_code);
    crate::arch::mirrors::fetch_mirrorlist(region_code).await
}

/// Fetch fallback mirrorlist when region selection was skipped
async fn fetch_fallback_mirrorlist() -> Result<String> {
    // Use empty region code to trigger fallback chain in fetch_mirrorlist
    crate::arch::mirrors::fetch_mirrorlist("").await
}

/// PCI vendor IDs of common network interface controllers and the
/// `linux-firmware-*` split that ships their firmware. Sorted by vendor ID
/// for grep-friendliness; extend by adding a row.
const NIC_VENDOR_FIRMWARE: &[(&str, &str)] = &[
    ("104c", "linux-firmware-ti"),
    ("10ec", "linux-firmware-realtek"),
    ("14c3", "linux-firmware-mediatek"),
    ("14e4", "linux-firmware-broadcom"),
    ("168c", "linux-firmware-atheros"),
    ("8086", "linux-firmware-intel"),
];

/// Firmware packages matching the detected hardware.
///
/// The `linux-firmware` meta package pulls every vendor split — NVIDIA GSP
/// images, the AMD/AMDGPU stacks and all wifi vendors — which is most of a
/// GiB of download and extraction for hardware the machine does not have.
/// Instead, install the splits matching the detected GPUs, AMD CPU platform
/// firmware and network interface vendors plus the small catch-alls. When no
/// GPU was detected at all, fall back to the full meta package: shipping a
/// system whose graphics firmware is missing is worse than a slower install.
fn firmware_packages(plan: &InstallPlan) -> Vec<String> {
    // Small and universal: the catch-all split, laptop speaker firmware and
    // the firmware index/licence file.
    let mut packages: Vec<String> = strings(&[
        "linux-firmware-other",
        "linux-firmware-cirrus",
        "linux-firmware-whence",
    ]);

    let mut wanted: HashSet<&str> = HashSet::new();
    for gpu in &plan.system_info.gpus {
        match gpu {
            GpuKind::Nvidia => {
                wanted.insert("linux-firmware-nvidia");
            }
            GpuKind::Amd => {
                wanted.insert("linux-firmware-amdgpu");
            }
            GpuKind::Intel => {
                wanted.insert("linux-firmware-intel");
            }
            GpuKind::Other(_) => {}
        }
    }

    // linux-firmware-amd is the AMD CPU/APU platform firmware (NPU, TEE,
    // Secure Processor). CPU microcode is a separate `amd-ucode` package
    // installed above. Keying on the CPU keeps AMD-GPU + Intel-CPU installs
    // from pulling platform firmware they will never use.
    if plan.system_info.has_amd_cpu {
        wanted.insert("linux-firmware-amd");
    }

    for vendor in &plan.system_info.network_vendor_ids {
        for (vid, pkg) in NIC_VENDOR_FIRMWARE {
            if vendor == vid {
                wanted.insert(pkg);
            }
        }
    }

    if plan.system_info.gpus.is_empty() {
        println!("No GPU detected; falling back to the full linux-firmware set");
        return vec!["linux-firmware".to_string()];
    }

    packages.extend(wanted.into_iter().map(String::from));
    packages.sort();
    packages
}

fn run_pacstrap(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let kernel = plan.kernel;
    let use_encryption = plan.storage.encryption().is_some();
    let use_plymouth = plan.use_plymouth;
    let minimal_mode = plan.minimal_mode;

    let mut packages: Vec<String> = vec!["base".to_string()];
    packages.extend(firmware_packages(plan));

    if plan.storage.filesystem().is_btrfs() {
        packages.push("btrfs-progs".to_string());
    }

    // Add kernel (headers are installed later alongside extra packages)
    packages.push(kernel.label().to_string());

    // CPU Microcode
    if plan.system_info.has_amd_cpu {
        println!("Detected AMD CPU, adding amd-ucode");
        packages.push("amd-ucode".to_string());
    }
    if plan.system_info.has_intel_cpu {
        println!("Detected Intel CPU, adding intel-ucode");
        packages.push("intel-ucode".to_string());
    }

    // GPU drivers are installed later in setup.rs after multilib is enabled,
    // allowing lib32-* packages to be installed properly.

    // Encryption support
    if use_encryption {
        println!("Encryption enabled; required packages will be installed inside chroot.");
    }

    // Plymouth support
    if use_plymouth && !minimal_mode {
        println!("Plymouth enabled; package will be installed after chroot.");
    }

    println!("Packages to install: {}", packages.join(" "));

    // Convert Vec<String> to Vec<&str> for pacstrap
    let packages_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();

    super::pacman::pacstrap("/mnt", &packages_refs, executor)?;

    Ok(())
}

#[cfg(test)]
mod firmware_tests {
    use super::firmware_packages;
    use crate::arch::engine::test_install_plan;
    use crate::arch::engine::{GpuKind, SystemInfo};

    fn plan_with(system_info: SystemInfo) -> crate::arch::engine::InstallPlan {
        let mut plan = test_install_plan();
        plan.system_info = system_info;
        plan
    }

    #[test]
    fn unknown_hardware_gets_the_full_meta_package() {
        // No GPU detected at all: do not gamble on missing graphics firmware.
        let plan = plan_with(SystemInfo::default());
        assert_eq!(firmware_packages(&plan), vec!["linux-firmware"]);
    }

    #[test]
    fn virtual_machine_without_passthrough_devices_skips_vendor_firmware() {
        // The e2e VM shape: a std VGA adapter and a virtio NIC, neither of
        // which maps to a firmware vendor split.
        let mut system_info = SystemInfo::default();
        system_info.gpus = vec![GpuKind::Other("QEMU Virtual Video".to_owned())];
        let plan = plan_with(system_info);

        let packages = firmware_packages(&plan);
        assert!(!packages.iter().any(|p| p.contains("nvidia")));
        assert!(!packages.iter().any(|p| p.contains("amd")));
        assert!(!packages.iter().any(|p| p.contains("intel")));
        assert!(packages.contains(&"linux-firmware-other".to_string()));
    }

    #[test]
    fn amd_cpu_with_amd_gpu_and_intel_nic_gets_each_split_for_its_own_reason() {
        // AMD GPU -> linux-firmware-amdgpu (graphics firmware).
        // AMD CPU  -> linux-firmware-amd (CPU/APU platform firmware).
        // Intel NIC -> linux-firmware-intel, Realtek NIC -> linux-firmware-realtek.
        let mut system_info = SystemInfo::default();
        system_info.has_amd_cpu = true;
        system_info.gpus = vec![GpuKind::Amd];
        system_info.network_vendor_ids = vec!["8086".to_owned(), "10ec".to_owned()];
        let plan = plan_with(system_info);

        let packages = firmware_packages(&plan);
        assert!(packages.contains(&"linux-firmware-amd".to_string()));
        assert!(packages.contains(&"linux-firmware-amdgpu".to_string()));
        assert!(packages.contains(&"linux-firmware-intel".to_string()));
        assert!(packages.contains(&"linux-firmware-realtek".to_string()));
        assert!(!packages.iter().any(|p| p.contains("nvidia")));
        assert!(!packages.iter().any(|p| p.contains("broadcom")));
    }

    #[test]
    fn amd_gpu_without_amd_cpu_skips_platform_firmware() {
        // Discrete AMD GPU + Intel CPU is a common combo. We must not pull
        // linux-firmware-amd, which is CPU/APU platform firmware.
        let mut system_info = SystemInfo::default();
        system_info.gpus = vec![GpuKind::Amd];
        let plan = plan_with(system_info);

        let packages = firmware_packages(&plan);
        assert!(packages.contains(&"linux-firmware-amdgpu".to_string()));
        assert!(!packages.contains(&"linux-firmware-amd".to_string()));
    }

    #[test]
    fn nvidia_gpu_gets_nvidia_firmware() {
        let mut system_info = SystemInfo::default();
        system_info.gpus = vec![GpuKind::Nvidia];
        let plan = plan_with(system_info);

        assert!(firmware_packages(&plan).contains(&"linux-firmware-nvidia".to_string()));
    }
}
