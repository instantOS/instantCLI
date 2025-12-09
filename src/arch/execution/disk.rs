use super::CommandExecutor;
use crate::arch::engine::{
    BootMode, DualBootPartitionPaths, DualBootPartitions, EspNeedsFormat, InstallContext,
    QuestionId,
};
use anyhow::{Context, Result};
use std::process::Command;

pub fn prepare_disk(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let disk = context
        .get_answer(&QuestionId::Disk)
        .context("No disk selected")?;

    // Extract device path from "path (size)" format if needed
    // The validation in DiskQuestion ensures it starts with /dev/
    // and we store the full string "path (size)" in the answer.
    // We need to extract just the path.
    let disk_path = disk.split('(').next().unwrap_or(disk).trim();

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
    swap_size_gb: u64,
) -> Result<()> {
    println!("Preparing dual boot installation...");

    // Get disk info from cached detection data
    let disks = context
        .get::<crate::arch::dualboot::DisksKey>()
        .context("Disk detection data not available - run dual boot detection first")?;

    let disk_info = disks
        .iter()
        .find(|d| d.device == disk_path)
        .context("Selected disk not found in detection data")?;

    // Find existing ESP (already detected with is_efi flag)
    let esp = disk_info
        .find_reusable_esp()
        .context("No suitable EFI partition found for dual boot (need >= 260MB ESP)")?;

    println!(
        "Reusing existing ESP: {} ({})",
        esp.device,
        crate::arch::dualboot::format_size(esp.size_bytes)
    );

    // Validate we have enough free space
    let available_space = disk_info.unpartitioned_space_bytes;
    if available_space < crate::arch::dualboot::MIN_LINUX_SIZE {
        anyhow::bail!(
            "Not enough free space: {} available, {} required",
            crate::arch::dualboot::format_size(available_space),
            crate::arch::dualboot::format_size(crate::arch::dualboot::MIN_LINUX_SIZE)
        );
    }

    // Create partitions in free space only
    // sfdisk can append partitions to existing partition table
    let (root_path, swap_path) =
        create_dualboot_partitions(disk_path, swap_size_gb, executor)?;

    // Store partition paths in context data store - CONVERGENCE POINT
    // This uses the data map which has interior mutability via Arc<Mutex>
    context.set::<DualBootPartitions>(DualBootPartitionPaths {
        root: root_path,
        boot: esp.device.clone(),
        swap: swap_path,
    });

    // Mark that ESP should NOT be reformatted (it's existing)
    context.set::<EspNeedsFormat>(false);

    // Now use the SAME formatting/mounting code as manual mode
    format_and_mount_partitions(context, executor)?;

    Ok(())
}

/// Create swap and root partitions in the unpartitioned space of a disk
///
/// Returns (root_path, swap_path) for the newly created partitions
fn create_dualboot_partitions(
    disk_path: &str,
    swap_size_gb: u64,
    executor: &CommandExecutor,
) -> Result<(String, String)> {
    println!("Creating partitions in free space...");

    // Use sfdisk to append partitions to existing table
    // The "+" prefix means "append after last partition"
    // We create: swap partition, then root partition (rest of space)
    let script = format!(
        "size={}G, type=S\n\
         type=L\n",
        swap_size_gb
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

    // Determine partition paths
    // After appending, we need to find the new partition numbers
    // We can use lsblk to get the latest partitions
    let (swap_path, root_path) = get_last_two_partitions(disk_path)?;

    println!("Created swap partition: {}", swap_path);
    println!("Created root partition: {}", root_path);

    Ok((root_path, swap_path))
}

/// Get the last two partition paths on a disk (swap, root order)
fn get_last_two_partitions(disk_path: &str) -> Result<(String, String)> {
    let output = std::process::Command::new("lsblk")
        .args(["-n", "-o", "NAME", "-r", disk_path])
        .output()
        .context("Failed to run lsblk")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let partitions: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with(disk_path.strip_prefix("/dev/").unwrap_or(disk_path)))
        .filter(|l| *l != disk_path.strip_prefix("/dev/").unwrap_or(disk_path))
        .collect();

    if partitions.len() < 2 {
        anyhow::bail!("Expected at least 2 partitions after creating dual boot partitions");
    }

    // Last two partitions are the newly created ones (swap, root)
    let swap_name = partitions[partitions.len() - 2];
    let root_name = partitions[partitions.len() - 1];

    Ok((format!("/dev/{}", swap_name), format!("/dev/{}", root_name)))
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
        let root_part = context
            .get_answer(&QuestionId::RootPartition)
            .context("Root partition not set")?;
        root_part.split('(').next().unwrap_or(root_part).trim().to_string()
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
        context
            .get_answer(&QuestionId::BootPartition)
            .map(|s| s.split('(').next().unwrap_or(s).trim().to_string())
    };

    if let Some(boot_path) = boot_path {
        // Check if we should format the ESP (false for dual boot reuse)
        let should_format = context.get::<EspNeedsFormat>().unwrap_or(true);

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
        executor.run(Command::new("mount").args(["--mkdir", &boot_path, "/mnt/boot"]))?;
    }

    // Handle Swap
    let swap_path = if let Some(ref paths) = dualboot_paths {
        Some(paths.swap.clone())
    } else {
        context
            .get_answer(&QuestionId::SwapPartition)
            .map(|s| s.split('(').next().unwrap_or(s).trim().to_string())
    };

    if let Some(swap_path) = swap_path {
        println!("Formatting Swap: {}", swap_path);
        executor.run(Command::new("mkswap").arg(&swap_path))?;
        println!("Activating Swap...");
        executor.run(Command::new("swapon").arg(&swap_path))?;
    }

    // Handle Home (only from answers, dual boot doesn't set this)
    if let Some(home_part) = context.get_answer(&QuestionId::HomePartition) {
        let home_path = home_part.split('(').next().unwrap_or(home_part).trim();
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
