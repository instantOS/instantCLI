use super::filesystem;
use crate::arch::engine::{
    BootMode, DualBootPartitions, EspNeedsFormat, FilesystemPlan, InstallContext, ManualPartitions,
};
use crate::arch::execution::CommandRunner;
use anyhow::{Context, Result};
use std::process::Command;

pub fn format_and_mount_partitions(
    context: &InstallContext,
    filesystem_plan: FilesystemPlan,
    manual_partitions: Option<&ManualPartitions>,
    boot_mode: &BootMode,
    executor: &dyn CommandRunner,
) -> Result<()> {
    println!("Formatting and mounting partitions...");

    let dualboot_paths = context.get::<DualBootPartitions>();

    let root_path = if let Some(ref paths) = dualboot_paths {
        paths.root.clone()
    } else {
        manual_partitions
            .context("manual partition plan missing")?
            .root
            .as_str()
            .to_owned()
    };

    println!("Formatting Root partition: {}", root_path);
    filesystem::format_root(filesystem_plan, &root_path, executor)?;

    println!("Mounting Root partition...");
    let has_separate_home = manual_partitions.is_some_and(|parts| parts.home.is_some());
    filesystem::mount_root(filesystem_plan, &root_path, !has_separate_home, executor)?;

    let boot_path = if let Some(ref paths) = dualboot_paths {
        Some(paths.boot.clone())
    } else {
        manual_partitions.map(|parts| parts.boot.as_str().to_owned())
    };

    if let Some(boot_path) = boot_path {
        let should_format = context.get::<EspNeedsFormat>().unwrap_or(true);

        let boot_mount_point = if dualboot_paths.is_some() {
            "/mnt/boot/efi"
        } else {
            "/mnt/boot"
        };

        if should_format {
            println!("Formatting Boot partition: {}", boot_path);
            filesystem::wipe_signatures(&boot_path, executor)?;
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
        let boot_type = match boot_mode {
            BootMode::UEFI64 | BootMode::UEFI32 => "vfat",
            BootMode::BIOS => "ext4",
        };
        executor.run(Command::new("mount").args([
            "--mkdir",
            "-t",
            boot_type,
            &boot_path,
            boot_mount_point,
        ]))?;
    }

    let swap_path = if let Some(ref paths) = dualboot_paths {
        Some(paths.swap.clone())
    } else {
        manual_partitions.and_then(|parts| parts.swap.as_ref().map(|path| path.as_str().to_owned()))
    };

    if let Some(swap_path) = swap_path {
        println!("Formatting Swap: {}", swap_path);
        filesystem::wipe_signatures(&swap_path, executor)?;
        executor.run(Command::new("mkswap").arg(&swap_path))?;
        println!("Activating Swap...");
        executor.run(Command::new("swapon").arg(&swap_path))?;
    }

    if let Some(home_path) = manual_partitions
        .and_then(|parts| parts.home.as_ref())
        .map(|path| path.as_str())
    {
        println!("Formatting Home partition: {}", home_path);
        filesystem::wipe_signatures(home_path, executor)?;
        executor.run(Command::new("mkfs.ext4").args(["-F", home_path]))?;
        println!("Mounting Home partition...");
        executor.run(Command::new("mount").args([
            "--mkdir",
            "-t",
            "ext4",
            home_path,
            "/mnt/home",
        ]))?;
    }

    Ok(())
}
