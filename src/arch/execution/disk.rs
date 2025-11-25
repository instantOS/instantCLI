use crate::arch::engine::{BootMode, InstallContext, QuestionId};
use anyhow::{Context, Result};
use std::process::Command;

pub fn prepare_disk(context: &InstallContext, dry_run: bool) -> Result<()> {
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

    // Unmount everything first just in case
    if dry_run {
        println!("[DRY RUN] umount -R /mnt || true");
        println!("[DRY RUN] swapoff -a || true");
    } else {
        let _ = Command::new("umount").args(["-R", "/mnt"]).status();
        let _ = Command::new("swapoff").args(["-a"]).status();
    }

    // Partitioning
    match boot_mode {
        BootMode::UEFI64 | BootMode::UEFI32 => {
            partition_uefi(disk_path, dry_run)?;
            format_uefi(disk_path, dry_run)?;
            mount_uefi(disk_path, dry_run)?;
        }
        BootMode::BIOS => {
            partition_bios(disk_path, dry_run)?;
            format_bios(disk_path, dry_run)?;
            mount_bios(disk_path, dry_run)?;
        }
    }

    Ok(())
}

fn partition_uefi(disk: &str, dry_run: bool) -> Result<()> {
    println!("Partitioning for UEFI...");

    // Layout:
    // 1. 1GiB EFI System
    // 2. 4GiB Swap (Fixed for now, could be dynamic)
    // 3. Rest Root

    let script = format!(
        "label: gpt\n\
         size=1G, type=U\n\
         size=4G, type=S\n\
         type=L\n"
    );

    if dry_run {
        println!(
            "[DRY RUN] echo '{}' | sfdisk {}",
            script.replace('\n', "\\n"),
            disk
        );
    } else {
        use std::io::Write;
        let mut child = Command::new("sfdisk")
            .arg(disk)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped()) // Capture output to avoid clutter
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(script.as_bytes())?;
        }

        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to partition disk {}", disk);
        }
    }

    // Wait for kernel to update partition table
    if !dry_run {
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

fn partition_bios(disk: &str, dry_run: bool) -> Result<()> {
    println!("Partitioning for BIOS...");

    // Layout:
    // 1. 4GiB Swap
    // 2. Rest Root

    let script = format!(
        "label: dos\n\
         size=4G, type=82\n\
         type=83\n"
    );

    if dry_run {
        println!(
            "[DRY RUN] echo '{}' | sfdisk {}",
            script.replace('\n', "\\n"),
            disk
        );
    } else {
        use std::io::Write;
        let mut child = Command::new("sfdisk")
            .arg(disk)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(script.as_bytes())?;
        }

        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to partition disk {}", disk);
        }
    }

    if !dry_run {
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

fn format_uefi(disk: &str, dry_run: bool) -> Result<()> {
    let p1 = get_part_path(disk, 1); // EFI
    let p2 = get_part_path(disk, 2); // Swap
    let p3 = get_part_path(disk, 3); // Root

    println!("Formatting partitions...");

    if dry_run {
        println!("[DRY RUN] mkfs.fat -F32 {}", p1);
        println!("[DRY RUN] mkswap {}", p2);
        println!("[DRY RUN] mkfs.ext4 -F {}", p3);
    } else {
        run_cmd("mkfs.fat", &["-F32", &p1])?;
        run_cmd("mkswap", &[&p2])?;
        run_cmd("mkfs.ext4", &["-F", &p3])?;
    }

    Ok(())
}

fn format_bios(disk: &str, dry_run: bool) -> Result<()> {
    let p1 = get_part_path(disk, 1); // Swap
    let p2 = get_part_path(disk, 2); // Root

    println!("Formatting partitions...");

    if dry_run {
        println!("[DRY RUN] mkswap {}", p1);
        println!("[DRY RUN] mkfs.ext4 -F {}", p2);
    } else {
        run_cmd("mkswap", &[&p1])?;
        run_cmd("mkfs.ext4", &["-F", &p2])?;
    }

    Ok(())
}

fn mount_uefi(disk: &str, dry_run: bool) -> Result<()> {
    let p1 = get_part_path(disk, 1); // EFI
    let p2 = get_part_path(disk, 2); // Swap
    let p3 = get_part_path(disk, 3); // Root

    println!("Mounting partitions...");

    if dry_run {
        println!("[DRY RUN] mount {} /mnt", p3);
        println!("[DRY RUN] mount --mkdir {} /mnt/boot", p1);
        println!("[DRY RUN] swapon {}", p2);
    } else {
        run_cmd("mount", &[&p3, "/mnt"])?;
        run_cmd("mount", &["--mkdir", &p1, "/mnt/boot"])?;
        run_cmd("swapon", &[&p2])?;
    }

    Ok(())
}

fn mount_bios(disk: &str, dry_run: bool) -> Result<()> {
    let p1 = get_part_path(disk, 1); // Swap
    let p2 = get_part_path(disk, 2); // Root

    println!("Mounting partitions...");

    if dry_run {
        println!("[DRY RUN] mount {} /mnt", p2);
        println!("[DRY RUN] swapon {}", p1);
    } else {
        run_cmd("mount", &[&p2, "/mnt"])?;
        run_cmd("swapon", &[&p1])?;
    }

    Ok(())
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd).args(args).status()?;
    if !status.success() {
        anyhow::bail!("Command failed: {} {:?}", cmd, args);
    }
    Ok(())
}
