use super::CommandExecutor;
use crate::arch::dualboot::{DisksKey, PartitionTableType};
use crate::arch::dualboot::parsing::get_free_regions;
use crate::arch::dualboot::types::MIN_ESP_SIZE;
use crate::arch::engine::{
    BootMode, DualBootPartitionPaths, DualBootPartitions, EspNeedsFormat, InstallContext,
    QuestionId,
};
use anyhow::{Context, Result};
use std::process::Command;

pub fn prepare_disk(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    // disk is now just the device path (e.g., "/dev/sda")
    let disk_path = context
        .get_answer(&QuestionId::Disk)
        .context("No disk selected")?;

    println!("Preparing disk: {}", disk_path);

    let boot_mode = &context.system_info.boot_mode;

    // Calculate swap size
    let ram_size_gb = get_total_ram_gb().unwrap_or(4);
    // Rule of thumb: At least 4GB, or equal to RAM for hibernation support
    let swap_size_gb = std::cmp::max(4, ram_size_gb);
    println!(
        "Detected RAM: {} GiB, setting Swap: {} GiB",
        ram_size_gb, swap_size_gb
    );

    // Partitioning
    let partitioning_method = context
        .get_answer(&QuestionId::PartitioningMethod)
        .map(|s| s.as_str())
        .unwrap_or("Automatic");

    if partitioning_method.contains("Dual Boot") {
        // Dual boot: create partitions in free space, reuse existing ESP
        prepare_dualboot_disk(context, executor, disk_path, swap_size_gb)?;
    } else if partitioning_method.contains("Manual") {
        // Manual: user already selected partitions via questions
        format_and_mount_partitions(context, executor)?;
    } else {
        // Automatic: full disk partitioning
        let use_encryption = context.get_answer_bool(QuestionId::UseEncryption);

        match (boot_mode, use_encryption) {
            (BootMode::UEFI64 | BootMode::UEFI32, false) => {
                partition_uefi(disk_path, executor, swap_size_gb)?;
                format_uefi(disk_path, executor)?;
                mount_uefi(disk_path, executor)?;
            }
            (BootMode::BIOS, false) => {
                partition_bios(disk_path, executor, swap_size_gb)?;
                format_bios(disk_path, executor)?;
                mount_bios(disk_path, executor)?;
            }
            (BootMode::UEFI64 | BootMode::UEFI32, true) => {
                partition_uefi_luks(disk_path, executor)?;
                format_luks(context, disk_path, executor, true, swap_size_gb)?;
                mount_luks(executor, disk_path)?;
            }
            (BootMode::BIOS, true) => {
                partition_bios_luks(disk_path, executor)?;
                format_luks(context, disk_path, executor, false, swap_size_gb)?;
                mount_luks(executor, disk_path)?;
            }
        }
    }

    Ok(())
}

/// Prepare disk for dual boot installation
///
/// This function:
/// 1. Finds existing ESP on the disk (reuses it without reformatting)
/// 2. Creates new partitions ONLY in unpartitioned space (swap + root)
/// 3. Stores partition paths in context for use by format_and_mount_partitions
fn prepare_dualboot_disk(
    context: &InstallContext,
    executor: &CommandExecutor,
    disk_path: &str,
    mut swap_size_gb: u64,
) -> Result<()> {
    println!("Preparing dual boot installation...");

    // Get disk info from cached detection data; fallback to fresh detection if missing
    let disks = if let Some(cached) = context.get::<DisksKey>() {
        cached.clone()
    } else {
        let detected = crate::arch::dualboot::detect_disks()
            .context("Disk detection data not available and re-detection failed")?;
        context.set::<DisksKey>(detected.clone());
        detected
    };

    let disk_info = disks
        .iter()
        .find(|d| d.device == disk_path)
        .context("Selected disk not found in detection data")?;

    // Find existing ESP (already detected with is_efi flag), or create one if missing
    let (esp_path, esp_needs_format) = if let Some(esp) = disk_info.find_reusable_esp() {
        println!(
            "Reusing existing ESP: {} ({})",
            esp.device,
            crate::arch::dualboot::format_size(esp.size_bytes)
        );
        (esp.device.clone(), false)
    } else {
        println!("No suitable ESP found (need >= 260MB). Creating a new EFI System Partition...");
        let new_esp = create_esp_partition(disk_path, disk_info, executor)?;
        println!("Created new ESP: {}", new_esp);
        (new_esp, true)
    };

    // Validate we have enough free space (contiguous region reported by sfdisk)
    // Also cap swap so it is never larger than half of root: swap <= root / 2 => swap <= free / 3
    let available_space = disk_info.max_contiguous_free_space_bytes;
    const GB: u64 = 1024 * 1024 * 1024;
    let mut swap_size_bytes = swap_size_gb * GB;

    if available_space <= crate::arch::dualboot::MIN_LINUX_SIZE {
        anyhow::bail!("Not enough contiguous free space for minimum root");
    }

    let swap_cap_by_ratio = available_space / 3; // ensures swap <= root/2
    let swap_cap_by_root_min =
        available_space.saturating_sub(crate::arch::dualboot::MIN_LINUX_SIZE);
    let swap_cap = swap_cap_by_ratio.min(swap_cap_by_root_min);

    if swap_size_bytes > swap_cap {
        swap_size_bytes = swap_cap;
        // Recompute swap_size_gb to align with the capped bytes (floor to GB, min 1GB)
        let adjusted_swap_gb = (swap_size_bytes / GB).max(1);
        println!(
            "Capping swap to {} (was {} GiB) to keep swap <= half of root",
            crate::arch::dualboot::format_size(adjusted_swap_gb * GB),
            swap_size_gb
        );
        swap_size_bytes = adjusted_swap_gb * GB;
        swap_size_gb = adjusted_swap_gb;
    }

    let min_required = crate::arch::dualboot::MIN_LINUX_SIZE + swap_size_bytes;
    // Allow a small 2 MiB alignment slack to account for rounding/alignment losses
    let alignment_slack = 2 * 1024 * 1024;

    if available_space + alignment_slack < min_required {
        anyhow::bail!(
            "Not enough contiguous free space: {} available, {} required ({} Root + {} Swap)",
            crate::arch::dualboot::format_size(available_space),
            crate::arch::dualboot::format_size(min_required),
            crate::arch::dualboot::format_size(crate::arch::dualboot::MIN_LINUX_SIZE),
            crate::arch::dualboot::format_size(swap_size_bytes)
        );
    }

    // Create partitions in free space only
    // sfdisk can append partitions to existing partition table
    let (root_path, swap_path) =
        create_dualboot_partitions(disk_path, swap_size_gb, disk_info.size_bytes, executor)?;

    // Store partition paths in context data store - CONVERGENCE POINT
    // This uses the data map which has interior mutability via Arc<Mutex>
    context.set::<DualBootPartitions>(DualBootPartitionPaths {
        root: root_path,
        boot: esp_path.clone(),
        swap: swap_path,
    });

    // Mark whether the ESP is new (needs format) or reused
    context.set::<EspNeedsFormat>(esp_needs_format);

    // Now use the SAME formatting/mounting code as manual mode
    format_and_mount_partitions(context, executor)?;

    Ok(())
}

/// Create an EFI System Partition if none is present
fn create_esp_partition(
    disk_path: &str,
    disk_info: &crate::arch::dualboot::DiskInfo,
    executor: &CommandExecutor,
) -> Result<String> {
    // Snapshot partitions BEFORE
    let partitions_before = get_current_partitions(disk_path)?;

    // We need at least MIN_ESP_SIZE contiguous space
    let esp_size_bytes = MIN_ESP_SIZE;
    let esp_sectors = esp_size_bytes.div_ceil(512);

    let regions = get_free_regions(disk_path, Some(disk_info.size_bytes))
        .context("Failed to get free regions for ESP creation")?;

    let region = regions
        .iter()
        .find(|r| r.sectors >= esp_sectors)
        .context("No free region large enough to create an EFI System Partition (need >= 260MB)")?;

    let start_sector = region.start;

    let type_code = match disk_info.partition_table {
        PartitionTableType::GPT => "c12a7328-f81f-11d2-ba4b-00a0c93ec93b",
        PartitionTableType::MBR | PartitionTableType::Unknown => "0xef",
    };

    let script = format!(
        "start={}, size={}, type={}\n",
        start_sector, esp_sectors, type_code
    );

    executor.run_with_input(
        Command::new("sfdisk").arg("--append").arg(disk_path),
        &script,
    )?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    // Snapshot partitions AFTER
    let partitions_after = get_current_partitions(disk_path)?;

    // Find new partition (difference set)
    let mut new_parts: Vec<String> = partitions_after
        .into_iter()
        .filter(|p| !partitions_before.contains(p))
        .collect();

    if new_parts.is_empty() {
        anyhow::bail!("Failed to identify newly created ESP partition");
    }

    // If multiple, pick the smallest (ESP should be the smallest addition)
    new_parts.sort_by_key(|p| get_partition_size_bytes(p).unwrap_or(u64::MAX));
    Ok(new_parts[0].clone())
}

/// Create swap and root partitions using optimal placement in free space
///
/// Returns (root_path, swap_path) for the newly created partitions
fn create_dualboot_partitions(
    disk_path: &str,
    swap_size_gb: u64,
    disk_size_bytes: u64,
    executor: &CommandExecutor,
) -> Result<(String, String)> {
    println!("Creating partitions in free space (optimal placement)...");

    // Snapshot partitions BEFORE
    let partitions_before = get_current_partitions(disk_path)?;

    // Get free regions to calculate optimal layout
    let regions = get_free_regions(disk_path, Some(disk_size_bytes))
        .context("Failed to get free space regions")?;

    if regions.is_empty() {
        anyhow::bail!("No free space regions detected!");
    }

    // Convert swap size to sectors (approx 512 bytes per sector)
    // We strictly use 512 for sector calculations as per sfdisk default/LBA
    let swap_size_bytes = swap_size_gb * 1024 * 1024 * 1024;
    let swap_sectors = swap_size_bytes.div_ceil(512);

    // 1. ALLOCATE SWAP
    // Strategy: First Fit (Find first hole large enough)
    let mut swap_start_sector = 0;
    let mut found_swap = false;

    // We clone regions to modify them as we allocate
    let mut available_regions = regions.clone();

    for region in available_regions.iter_mut() {
        if region.sectors >= swap_sectors {
            swap_start_sector = region.start;

            // Update the region to reflect used space
            // This allows Root to use the remainder of THIS same region if it's still the largest
            region.start += swap_sectors;
            region.sectors -= swap_sectors;
            region.size_bytes = region.size_bytes.saturating_sub(swap_size_bytes);

            found_swap = true;
            break;
        }
    }

    if !found_swap {
        anyhow::bail!(
            "Could not find a contiguous free region large enough for Swap ({} GB)",
            swap_size_gb
        );
    }

    // 2. ALLOCATE ROOT
    // Strategy: Best Fit (Largest remaining hole)
    // available_regions now has the Swap space removed

    let root_region = available_regions
        .iter()
        .max_by_key(|r| r.sectors)
        .context("No free regions left for Root partition")?;

    let root_start_sector = root_region.start;
    let root_size_sectors = root_region.sectors;

    // Verify Root Size
    let root_size_bytes = root_size_sectors * 512;
    if root_size_bytes < crate::arch::dualboot::MIN_LINUX_SIZE {
        anyhow::bail!(
            "Largest remaining free space is too small for Root: {}",
            crate::arch::dualboot::format_size(root_size_bytes)
        );
    }

    println!("Placement:");
    println!(
        "  Swap: Start Sector {}, Size {} GB",
        swap_start_sector, swap_size_gb
    );
    println!(
        "  Root: Start Sector {}, Size {} (approx)",
        root_start_sector,
        crate::arch::dualboot::format_size(root_size_bytes)
    );

    // 3. GENERATE SCRIPT
    // Use explicit start sectors to guarantee placement
    // Note: We use size in sectors with 'S' suffix or implicitly if we provide start?
    // sfdisk script: start=..., size=..., type=...

    let script = format!(
        "start={}, size={}, type=S\n\
         start={}, size={}, type=L\n",
        swap_start_sector, swap_sectors, root_start_sector, root_size_sectors
    );

    // sfdisk --append appends to existing partition table
    executor.run_with_input(
        Command::new("sfdisk").arg("--append").arg(disk_path),
        &script,
    )?;

    // Wait for kernel to update partition table
    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    // Snapshot partitions AFTER
    let partitions_after = get_current_partitions(disk_path)?;

    // Find new partitions
    let new_partitions: Vec<String> = partitions_after
        .into_iter()
        .filter(|p| !partitions_before.contains(p))
        .collect();

    if new_partitions.len() < 2 {
        anyhow::bail!(
            "Expected 2 new partitions, found {}: {:?}",
            new_partitions.len(),
            new_partitions
        );
    }

    // 4. IDENTIFY PARTITIONS
    // We cannot assume creating order determines partition number (MBR logical partitions etc)
    // We identify based on SIZE and TYPE

    let mut swap_path = String::new();
    let root_path: String;

    for p in &new_partitions {
        let size = get_partition_size_bytes(p)?;

        // Check if it matches Swap size (within margin)
        // AND check if type is swap?
        // Let's rely on size + simple heuristic first.
        // Swap is small (~4-32GB), Root is large (Rest of disk).

        // Is it the Swap we just created?
        // Abs diff between `size` and `swap_size_bytes`
        let diff = (size as i64 - swap_size_bytes as i64).abs();
        let margin = (swap_size_bytes / 20) as i64; // 5% margin

        if diff < margin {
            // Looks like Swap
            if swap_path.is_empty() {
                swap_path = p.clone();
            } else {
                // Ambiguity?
                // If Swap and Root are same size?? Unlikely given constraints (Root > 10G, Swap ~4-32G)
                // If they are similar size, we might be confused.
                // But Root should be "Maximize", so likely larger unless disk is small.
                // Just take first match?
            }
        }
    }

    // If we haven't identified both, use fallback or stricter logic.
    if swap_path.is_empty() {
        // Fallback: The smaller one is Swap?
        // Or sort by size?
        let mut sorted_by_size = new_partitions.clone();
        sorted_by_size.sort_by_key(|p| get_partition_size_bytes(p).unwrap_or(0));

        // Smaller = Swap, Larger = Root
        swap_path = sorted_by_size[0].clone();
        let assumed_root = sorted_by_size[1].clone();
        root_path = assumed_root.clone();

        println!(
            "Warning: Could not identify partitions by exact size match. Assuming smaller ({}) is Swap and larger ({}) is Root.",
            swap_path, assumed_root
        );
    } else {
        // Find Root (the one that is not Swap)
        root_path = new_partitions
            .iter()
            .find(|p| **p != swap_path)
            .unwrap()
            .clone();
    }

    // Verify identification
    let identified_swap_size = get_partition_size_bytes(&swap_path)?;
    println!(
        "Identified Swap: {} ({})",
        swap_path,
        crate::arch::dualboot::format_size(identified_swap_size)
    );
    println!(
        "Identified Root: {} ({})",
        root_path,
        crate::arch::dualboot::format_size(get_partition_size_bytes(&root_path)?)
    );
    println!(
        "Created root partition: {} ({})",
        root_path,
        crate::arch::dualboot::format_size(root_size_bytes)
    );

    Ok((root_path, swap_path))
}

/// Helper to get partition size in bytes
fn get_partition_size_bytes(device_path: &str) -> Result<u64> {
    let output = std::process::Command::new("lsblk")
        .args(["-n", "-o", "SIZE", "-b", device_path])
        .output()
        .context("Failed to get partition size")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse()
        .context("Failed to parse partition size")
}

/// Get list of current full partition paths on disk (e.g. ["/dev/sda1", "/dev/sda2"])
fn get_current_partitions(disk_path: &str) -> Result<std::collections::HashSet<String>> {
    let output = std::process::Command::new("lsblk")
        .args(["-n", "-o", "NAME", "-r", disk_path])
        .output()
        .context("Failed to run lsblk")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let disk_name = disk_path.strip_prefix("/dev/").unwrap_or(disk_path);

    let partitions: std::collections::HashSet<String> = stdout
        .lines()
        .filter(|l| l.starts_with(disk_name))
        .filter(|l| *l != disk_name) // Exclude the disk itself
        .map(|name| format!("/dev/{}", name))
        .collect();

    Ok(partitions)
}

/// Format and mount partitions based on paths stored in context
///
/// This is the CONVERGENCE POINT - used by both manual and dual boot modes
/// For dual boot: partition paths come from DualBootPartitions data key
/// For manual: partition paths come from QuestionId answers
fn format_and_mount_partitions(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Formatting and mounting partitions...");

    let boot_mode = &context.system_info.boot_mode;

    // Get partition paths - check dualboot data first, then answers
    let dualboot_paths = context.get::<DualBootPartitions>();

    let root_path = if let Some(ref paths) = dualboot_paths {
        paths.root.clone()
    } else {
        // Answer is now just the device path (e.g., "/dev/sda1")
        context
            .get_answer(&QuestionId::RootPartition)
            .context("Root partition not set")?
            .to_string()
    };

    // Format and mount root
    println!("Formatting Root partition: {}", root_path);
    executor.run(Command::new("mkfs.ext4").args(["-F", &root_path]))?;

    println!("Mounting Root partition...");
    executor.run(Command::new("mount").args([&root_path, "/mnt"]))?;

    // Handle Boot/EFI
    let boot_path = if let Some(ref paths) = dualboot_paths {
        Some(paths.boot.clone())
    } else {
        // Answer is now just the device path (e.g., "/dev/sda1")
        context
            .get_answer(&QuestionId::BootPartition)
            .map(|s| s.to_string())
    };

    if let Some(boot_path) = boot_path {
        // Check if we should format the ESP (false for dual boot reuse)
        let should_format = context.get::<EspNeedsFormat>().unwrap_or(true);

        // Dual boot: mount ESP at /boot/efi to avoid clobbering existing contents like amd-ucode
        let boot_mount_point = if dualboot_paths.is_some() {
            "/mnt/boot/efi"
        } else {
            "/mnt/boot"
        };

        if should_format {
            println!("Formatting Boot partition: {}", boot_path);
            match boot_mode {
                BootMode::UEFI64 | BootMode::UEFI32 => {
                    executor.run(Command::new("mkfs.fat").args(["-F32", &boot_path]))?;
                }
                BootMode::BIOS => {
                    executor.run(Command::new("mkfs.ext4").args(["-F", &boot_path]))?;
                }
            }
        } else {
            println!(
                "Reusing existing Boot partition: {} (not reformatting)",
                boot_path
            );
        }

        println!("Mounting Boot partition...");
        executor.run(Command::new("mount").args(["--mkdir", &boot_path, boot_mount_point]))?;
    }

    // Handle Swap
    let swap_path = if let Some(ref paths) = dualboot_paths {
        Some(paths.swap.clone())
    } else {
        // Answer is now just the device path (e.g., "/dev/sda1")
        context
            .get_answer(&QuestionId::SwapPartition)
            .map(|s| s.to_string())
    };

    if let Some(swap_path) = swap_path {
        println!("Formatting Swap: {}", swap_path);
        executor.run(Command::new("mkswap").arg(&swap_path))?;
        println!("Activating Swap...");
        executor.run(Command::new("swapon").arg(&swap_path))?;
    }

    // Handle Home (only from answers, dual boot doesn't set this)
    // Answer is now just the device path (e.g., "/dev/sda3")
    if let Some(home_path) = context.get_answer(&QuestionId::HomePartition) {
        println!("Formatting Home partition: {}", home_path);
        executor.run(Command::new("mkfs.ext4").args(["-F", home_path]))?;
        println!("Mounting Home partition...");
        executor.run(Command::new("mount").args(["--mkdir", home_path, "/mnt/home"]))?;
    }

    Ok(())
}

fn partition_uefi_luks(disk: &str, executor: &CommandExecutor) -> Result<()> {
    println!("Partitioning for UEFI with Encryption...");

    // Layout:
    // 1. 1GiB EFI System
    // 2. Rest LUKS
    let script = "label: gpt\n\
         size=1G, type=U\n\
         type=L\n";

    executor.run_with_input(Command::new("sfdisk").arg(disk), script)?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

fn partition_bios_luks(disk: &str, executor: &CommandExecutor) -> Result<()> {
    println!("Partitioning for BIOS with Encryption...");

    // Layout:
    // 1. 1GiB Boot
    // 2. Rest LUKS
    let script = "label: dos\n\
         size=1G, type=83\n\
         type=83\n";

    executor.run_with_input(Command::new("sfdisk").arg(disk), script)?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

fn format_luks(
    context: &InstallContext,
    disk: &str,
    executor: &CommandExecutor,
    is_uefi: bool,
    swap_size_gb: u64,
) -> Result<()> {
    let p1 = get_part_path(disk, 1); // EFI or Boot
    let p2 = get_part_path(disk, 2); // LUKS Container

    let password = context
        .get_answer(&QuestionId::EncryptionPassword)
        .context("Encryption password not set")?;

    println!("Formatting partitions (LVM on LUKS)...");

    // 1. Format Boot/EFI
    if is_uefi {
        executor.run(Command::new("mkfs.fat").args(["-F32", &p1]))?;
    } else {
        executor.run(Command::new("mkfs.ext4").args(["-F", &p1]))?;
    }

    // 2. Setup LUKS
    println!("Setting up LUKS container on {}...", p2);
    // echo -n "password" | cryptsetup luksFormat /dev/sdX2 -
    // Note: -q for batch mode (suppress confirmation)
    let mut cmd = Command::new("cryptsetup");
    cmd.arg("-q").arg("luksFormat").arg(&p2).arg("-");
    executor.run_with_input(&mut cmd, password)?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    // 3. Open LUKS
    println!("Opening LUKS container...");
    // echo -n "password" | cryptsetup open /dev/sdX2 cryptlvm -
    let mut cmd_open = Command::new("cryptsetup");
    cmd_open.arg("open").arg(&p2).arg("cryptlvm").arg("-");
    executor.run_with_input(&mut cmd_open, password)?;

    // 4. LVM Setup
    println!("Setting up LVM...");
    // pvcreate /dev/mapper/cryptlvm
    executor.run(Command::new("pvcreate").arg("/dev/mapper/cryptlvm"))?;

    // vgcreate instantOS /dev/mapper/cryptlvm
    executor.run(Command::new("vgcreate").args(["instantOS", "/dev/mapper/cryptlvm"]))?;

    // lvcreate -L 8G instantOS -n swap
    executor.run(Command::new("lvcreate").args([
        "-L",
        &format!("{}G", swap_size_gb),
        "instantOS",
        "-n",
        "swap",
    ]))?;

    // lvcreate -l 100%FREE instantOS -n root
    executor.run(Command::new("lvcreate").args(["-l", "100%FREE", "instantOS", "-n", "root"]))?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        executor.run(Command::new("vgchange").args(["-ay", "instantOS"]))?;
    }

    // 5. Format LVs
    println!("Formatting Logical Volumes...");
    executor.run(Command::new("mkswap").arg("/dev/instantOS/swap"))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", "/dev/instantOS/root"]))?;

    Ok(())
}

fn mount_luks(executor: &CommandExecutor, disk: &str) -> Result<()> {
    println!("Mounting LVM volumes...");

    // Mount root
    executor.run(Command::new("mount").args(["/dev/instantOS/root", "/mnt"]))?;

    // Mount boot
    let p1 = get_part_path(disk, 1);
    executor.run(Command::new("mount").args(["--mkdir", &p1, "/mnt/boot"]))?;

    // Swapon
    executor.run(Command::new("swapon").arg("/dev/instantOS/swap"))?;

    Ok(())
}

fn get_total_ram_gb() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: u64 = parts[1].parse().ok()?;
                // Convert KB to GiB (KB / 1024 / 1024)
                // We round up to nearest GB
                return Some(kb.div_ceil(1024 * 1024));
            }
        }
    }
    None
}

fn partition_uefi(disk: &str, executor: &CommandExecutor, swap_size_gb: u64) -> Result<()> {
    println!("Partitioning for UEFI...");

    // Layout:
    // 1. 1GiB EFI System
    // 2. Swap (Dynamic size)
    // 3. Rest Root

    let script = format!(
        "label: gpt\n\
         size=1G, type=U\n\
         size={}G, type=S\n\
         type=L\n",
        swap_size_gb
    );

    executor.run_with_input(Command::new("sfdisk").arg(disk), &script)?;

    // Wait for kernel to update partition table
    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

fn partition_bios(disk: &str, executor: &CommandExecutor, swap_size_gb: u64) -> Result<()> {
    println!("Partitioning for BIOS...");

    // Layout:
    // 1. Swap (Dynamic size)
    // 2. Rest Root

    let script = format!(
        "label: dos\n\
         size={}G, type=82\n\
         type=83\n",
        swap_size_gb
    );

    executor.run_with_input(Command::new("sfdisk").arg(disk), &script)?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

pub fn get_part_path(disk: &str, part_num: u32) -> String {
    // Handle nvme0n1 -> nvme0n1p1, sda -> sda1
    if disk.chars().last().unwrap_or(' ').is_numeric() {
        format!("{}p{}", disk, part_num)
    } else {
        format!("{}{}", disk, part_num)
    }
}

fn format_uefi(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1); // EFI
    let p2 = get_part_path(disk, 2); // Swap
    let p3 = get_part_path(disk, 3); // Root

    println!("Formatting partitions...");

    executor.run(Command::new("mkfs.fat").args(["-F32", &p1]))?;
    executor.run(Command::new("mkswap").arg(&p2))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", &p3]))?;

    Ok(())
}

fn format_bios(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1); // Swap
    let p2 = get_part_path(disk, 2); // Root

    println!("Formatting partitions...");

    executor.run(Command::new("mkswap").arg(&p1))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", &p2]))?;

    Ok(())
}

fn mount_uefi(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1); // EFI
    let p2 = get_part_path(disk, 2); // Swap
    let p3 = get_part_path(disk, 3); // Root

    println!("Mounting partitions...");

    executor.run(Command::new("mount").args([&p3, "/mnt"]))?;
    executor.run(Command::new("mount").args(["--mkdir", &p1, "/mnt/boot"]))?;

    // We activate swap here so that genfstab can automatically detect it
    // and add it to the generated /etc/fstab in the next step.
    executor.run(Command::new("swapon").arg(&p2))?;

    Ok(())
}

fn mount_bios(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1); // Swap
    let p2 = get_part_path(disk, 2); // Root

    println!("Mounting partitions...");

    executor.run(Command::new("mount").args([&p2, "/mnt"]))?;

    // We activate swap here so that genfstab can automatically detect it
    // and add it to the generated /etc/fstab in the next step.
    executor.run(Command::new("swapon").arg(&p1))?;

    Ok(())
}
