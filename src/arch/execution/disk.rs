use super::CommandExecutor;
use crate::arch::engine::{BootMode, InstallContext, QuestionId};
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

    if partitioning_method.contains("Manual") {
        prepare_manual_disk(context, executor)?;
    } else {
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

fn prepare_manual_disk(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Preparing manual partitions...");

    let root_part = context
        .get_answer(&QuestionId::RootPartition)
        .context("Root partition not selected")?;
    let root_path = root_part.split('(').next().unwrap_or(root_part).trim();

    let boot_mode = &context.system_info.boot_mode;

    // Format Root
    println!("Formatting Root partition: {}", root_path);
    executor.run(Command::new("mkfs.ext4").args(["-F", root_path]))?;

    // Mount Root
    println!("Mounting Root partition...");
    executor.run(Command::new("mount").args([root_path, "/mnt"]))?;

    // Handle Boot/EFI
    if let Some(boot_part) = context.get_answer(&QuestionId::BootPartition) {
        let boot_path = boot_part.split('(').next().unwrap_or(boot_part).trim();
        println!("Formatting Boot partition: {}", boot_path);

        // If UEFI, it must be FAT32. If BIOS, usually ext4 or just a directory on root.
        // But if they selected a separate boot partition, we should format it.
        // For UEFI it is mandatory.
        match boot_mode {
            BootMode::UEFI64 | BootMode::UEFI32 => {
                executor.run(Command::new("mkfs.fat").args(["-F32", boot_path]))?;
            }
            BootMode::BIOS => {
                // For BIOS, a separate boot partition is usually ext4
                executor.run(Command::new("mkfs.ext4").args(["-F", boot_path]))?;
            }
        }

        println!("Mounting Boot partition...");
        executor.run(Command::new("mount").args(["--mkdir", boot_path, "/mnt/boot"]))?;
    }

    // Handle Swap
    if let Some(swap_part) = context.get_answer(&QuestionId::SwapPartition) {
        let swap_path = swap_part.split('(').next().unwrap_or(swap_part).trim();
        println!("Formatting Swap partition: {}", swap_path);
        executor.run(Command::new("mkswap").arg(swap_path))?;
        println!("Activating Swap...");
        executor.run(Command::new("swapon").arg(swap_path))?;
    }

    // Handle Home
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
