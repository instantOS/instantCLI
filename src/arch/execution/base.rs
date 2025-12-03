use super::CommandExecutor;
use crate::arch::engine::{InstallContext, QuestionId};
use anyhow::{Context, Result};

pub async fn install_base(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Setting up mirrors...");
    setup_mirrors(context, executor).await?;

    println!("Configuring pacman settings...");
    crate::common::pacman::configure_pacman_settings(None, executor.dry_run).await?;

    println!("Installing base system...");
    run_pacstrap(context, executor)?;

    Ok(())
}

async fn setup_mirrors(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let region_name = context
        .get_answer(&QuestionId::MirrorRegion)
        .context("No mirror region selected")?;

    println!("Selected region: {}", region_name);

    if executor.dry_run {
        println!("[DRY RUN] Fetching mirrorlist for region: {}", region_name);
        println!("[DRY RUN] Writing to /etc/pacman.d/mirrorlist");
        return Ok(());
    }

    // Fetch region map to get code
    let regions = crate::arch::mirrors::fetch_mirror_regions().await?;
    let region_code = regions
        .get(region_name)
        .context(format!("Could not find code for region: {}", region_name))?;

    println!("Fetching mirrors for code: {}", region_code);
    let mirrorlist = crate::arch::mirrors::fetch_mirrorlist(region_code).await?;

    // Write to file
    std::fs::write("/etc/pacman.d/mirrorlist", mirrorlist)?;
    println!("Mirrors updated.");

    Ok(())
}

fn run_pacstrap(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    // Get selected kernel or default to "linux"
    let kernel = context
        .get_answer(&QuestionId::Kernel)
        .map(|s| s.as_str())
        .unwrap_or("linux");

    let mut packages_owned: Vec<String> = vec![
        "base".to_string(),
        kernel.to_string(),
        format!("{}-headers", kernel),
        "linux-firmware".to_string(),
        "vim".to_string(),
        "nano".to_string(),
        "networkmanager".to_string(), // Essential for networking
        "grub".to_string(),           // Bootloader
        "efibootmgr".to_string(),     // Required for GRUB on UEFI
        "os-prober".to_string(),      // Detect other OSes
        // Dependencies for instantCLI and menus (required for chroot steps)
        "git".to_string(),
        "libgit2".to_string(),
        "fzf".to_string(),
        "gum".to_string(),
        "base-devel".to_string(),
    ];

    // CPU Microcode
    if context.system_info.has_amd_cpu {
        println!("Detected AMD CPU, adding amd-ucode");
        packages_owned.push("amd-ucode".to_string());
    }
    if context.system_info.has_intel_cpu {
        println!("Detected Intel CPU, adding intel-ucode");
        packages_owned.push("intel-ucode".to_string());
    }

    // GPU Drivers
    let mut added_nvidia = false;
    for gpu in &context.system_info.gpus {
        match gpu {
            crate::arch::engine::GpuKind::Nvidia => {
                if !added_nvidia {
                    println!("Detected NVIDIA GPU, adding drivers");
                    match kernel {
                        "linux" => {
                            packages_owned.push("nvidia".to_string());
                        }
                        "linux-lts" => {
                            println!("LTS kernel selected, using nvidia-lts");
                            packages_owned.push("nvidia-lts".to_string());
                        }
                        _ => {
                            // For other custom kernels (zen, etc), we need dkms
                            println!("Custom kernel selected ({}), using nvidia-dkms", kernel);
                            packages_owned.push("nvidia-dkms".to_string());
                            packages_owned.push("dkms".to_string());
                        }
                    }
                    packages_owned.push("nvidia-utils".to_string());
                    packages_owned.push("nvidia-settings".to_string());
                    added_nvidia = true;
                }
            }
            _ => {
                for pkg in gpu.get_driver_packages() {
                    println!("Adding driver package: {}", pkg);
                    packages_owned.push(pkg.to_string());
                }
            }
        }
    }

    // Encryption support
    if context.get_answer_bool(QuestionId::UseEncryption) {
        println!("Encryption enabled, adding lvm2 and cryptsetup");
        packages_owned.push("lvm2".to_string());
        packages_owned.push("cryptsetup".to_string());
    }

    // Plymouth support
    if context.get_answer_bool(QuestionId::UsePlymouth)
        && !context.get_answer_bool(QuestionId::MinimalMode)
    {
        println!("Plymouth enabled, adding plymouth package");
        packages_owned.push("plymouth".to_string());
    }

    println!("Packages to install: {}", packages_owned.join(" "));

    // Convert Vec<String> to Vec<&str> for pacstrap
    let packages_refs: Vec<&str> = packages_owned.iter().map(|s| s.as_str()).collect();

    super::pacman::pacstrap("/mnt", &packages_refs, executor)?;

    Ok(())
}
