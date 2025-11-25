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
    match boot_mode {
        BootMode::UEFI64 | BootMode::UEFI32 => {
            partition_uefi(disk_path, executor, swap_size_gb)?;
            format_uefi(disk_path, executor)?;
            mount_uefi(disk_path, executor)?;
        }
        BootMode::BIOS => {
            partition_bios(disk_path, executor, swap_size_gb)?;
            format_bios(disk_path, executor)?;
            mount_bios(disk_path, executor)?;
        }
    }

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
                return Some((kb + 1024 * 1024 - 1) / (1024 * 1024));
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
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

fn get_part_path(disk: &str, part_num: u32) -> String {
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
