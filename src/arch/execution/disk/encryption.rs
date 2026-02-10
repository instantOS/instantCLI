use super::util::get_part_path;
use crate::arch::engine::{InstallContext, QuestionId};
use crate::arch::execution::CommandExecutor;
use anyhow::{Context, Result};
use std::process::Command;

pub fn partition_uefi_luks(disk: &str, executor: &CommandExecutor) -> Result<()> {
    println!("Partitioning for UEFI with Encryption...");

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

pub fn partition_bios_luks(disk: &str, executor: &CommandExecutor) -> Result<()> {
    println!("Partitioning for BIOS with Encryption...");

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

pub fn format_luks(
    context: &InstallContext,
    disk: &str,
    executor: &CommandExecutor,
    is_uefi: bool,
    swap_size_gb: u64,
) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);

    let password = context
        .get_answer(&QuestionId::EncryptionPassword)
        .context("Encryption password not set")?;

    println!("Formatting partitions (LVM on LUKS)...");

    if is_uefi {
        executor.run(Command::new("mkfs.fat").args(["-F32", &p1]))?;
    } else {
        executor.run(Command::new("mkfs.ext4").args(["-F", &p1]))?;
    }

    println!("Setting up LUKS container on {}...", p2);
    let mut cmd = Command::new("cryptsetup");
    cmd.arg("-q").arg("luksFormat").arg(&p2).arg("-");
    executor.run_with_input(&mut cmd, password)?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    println!("Opening LUKS container...");
    let mut cmd_open = Command::new("cryptsetup");
    cmd_open.arg("open").arg(&p2).arg("cryptlvm").arg("-");
    executor.run_with_input(&mut cmd_open, password)?;

    println!("Setting up LVM...");
    executor.run(Command::new("pvcreate").arg("/dev/mapper/cryptlvm"))?;
    executor.run(Command::new("vgcreate").args(["instantOS", "/dev/mapper/cryptlvm"]))?;

    executor.run(Command::new("lvcreate").args([
        "-L",
        &format!("{}G", swap_size_gb),
        "instantOS",
        "-n",
        "swap",
    ]))?;

    executor.run(Command::new("lvcreate").args(["-l", "100%FREE", "instantOS", "-n", "root"]))?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        executor.run(Command::new("vgchange").args(["-ay", "instantOS"]))?;
    }

    println!("Formatting Logical Volumes...");
    executor.run(Command::new("mkswap").arg("/dev/instantOS/swap"))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", "/dev/instantOS/root"]))?;

    Ok(())
}

pub fn mount_luks(executor: &CommandExecutor, disk: &str) -> Result<()> {
    println!("Mounting LVM volumes...");

    executor.run(Command::new("mount").args(["/dev/instantOS/root", "/mnt"]))?;

    let p1 = get_part_path(disk, 1);
    executor.run(Command::new("mount").args(["--mkdir", &p1, "/mnt/boot"]))?;

    executor.run(Command::new("swapon").arg("/dev/instantOS/swap"))?;

    Ok(())
}
