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
    // Check if a region was selected (question may have been skipped if fetch failed)
    let region_name = context.get_answer(&QuestionId::MirrorRegion);

    if executor.dry_run {
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

fn run_pacstrap(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    // Get selected kernel or default to "linux"
    let kernel = context
        .get_answer(&QuestionId::Kernel)
        .map(|s| s.as_str())
        .unwrap_or("linux");
    let use_encryption = context.get_answer_bool(QuestionId::UseEncryption);
    let use_plymouth = context.get_answer_bool(QuestionId::UsePlymouth);
    let minimal_mode = context.get_answer_bool(QuestionId::MinimalMode);

    // NOTE: libgit2 is required for /usr/bin/ins inside the chroot. The host ISO has it,
    // but pacstrap's base set does not. Install it up front to avoid chroot-time failures
    // like: "libgit2.so.1.9: cannot open shared object file".
    let mut packages: Vec<String> = vec!["base", "linux-firmware", "libgit2"]
        .into_iter()
        .map(String::from)
        .collect();

    // Add kernel (headers are installed later alongside extra packages)
    packages.push(kernel.to_string());

    // CPU Microcode
    if context.system_info.has_amd_cpu {
        println!("Detected AMD CPU, adding amd-ucode");
        packages.push("amd-ucode".to_string());
    }
    if context.system_info.has_intel_cpu {
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
