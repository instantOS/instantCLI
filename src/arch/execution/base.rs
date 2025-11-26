use super::CommandExecutor;
use crate::arch::engine::{InstallContext, QuestionId};
use anyhow::{Context, Result};
use std::process::Command;

pub async fn install_base(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Setting up mirrors...");
    setup_mirrors(context, executor).await?;

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
    let mut packages = vec![
        "base",
        "linux",
        "linux-headers",
        "linux-firmware",
        "vim",
        "nano",
        "networkmanager", // Essential for networking
        "grub",           // Bootloader
        "efibootmgr",     // Required for GRUB on UEFI
        "os-prober",      // Detect other OSes
        "sway",
    ];

    // CPU Microcode
    if context.system_info.has_amd_cpu {
        println!("Detected AMD CPU, adding amd-ucode");
        packages.push("amd-ucode");
    }
    if context.system_info.has_intel_cpu {
        println!("Detected Intel CPU, adding intel-ucode");
        packages.push("intel-ucode");
    }

    // NVIDIA GPU (if requested by plan, though often better in post-install)
    // Plan says: "check if nvidia gpu is present, if yes, add nvidia package to the list"
    if context.system_info.has_nvidia_gpu {
        println!("Detected NVIDIA GPU, adding nvidia");
        packages.push("nvidia");
    }

    println!("Packages to install: {}", packages.join(" "));

    let mut cmd = Command::new("pacstrap");
    cmd.arg("/mnt");
    cmd.args(&packages);

    executor.run(&mut cmd)?;

    Ok(())
}
