use super::CommandRunner;
use crate::arch::engine::InstallPlan;
use anyhow::{Context, Result};

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

fn run_pacstrap(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let kernel = plan.kernel;
    let use_encryption = plan.storage.encryption().is_some();
    let use_plymouth = plan.use_plymouth;
    let minimal_mode = plan.minimal_mode;

    let mut packages: Vec<String> = vec!["base", "linux-firmware"]
        .into_iter()
        .map(String::from)
        .collect();

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
