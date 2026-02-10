use crate::arch::engine::{
    BootMode, DualBootPartitions, EspNeedsFormat, InstallContext, QuestionId,
};
use crate::arch::execution::CommandExecutor;
use anyhow::{Context, Result};
use std::process::Command;

pub fn format_and_mount_partitions(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Formatting and mounting partitions...");

    let boot_mode = &context.system_info.boot_mode;

    let dualboot_paths = context.get::<DualBootPartitions>();

    let root_path = if let Some(ref paths) = dualboot_paths {
        paths.root.clone()
    } else {
        context
            .get_answer(&QuestionId::RootPartition)
            .context("Root partition not set")?
            .to_string()
    };

    println!("Formatting Root partition: {}", root_path);
    executor.run(Command::new("mkfs.ext4").args(["-F", &root_path]))?;

    println!("Mounting Root partition...");
    executor.run(Command::new("mount").args([&root_path, "/mnt"]))?;

    let boot_path = if let Some(ref paths) = dualboot_paths {
        Some(paths.boot.clone())
    } else {
        context
            .get_answer(&QuestionId::BootPartition)
            .map(|s| s.to_string())
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

    let swap_path = if let Some(ref paths) = dualboot_paths {
        Some(paths.swap.clone())
    } else {
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

    if let Some(home_path) = context.get_answer(&QuestionId::HomePartition) {
        println!("Formatting Home partition: {}", home_path);
        executor.run(Command::new("mkfs.ext4").args(["-F", home_path]))?;
        println!("Mounting Home partition...");
        executor.run(Command::new("mount").args(["--mkdir", home_path, "/mnt/home"]))?;
    }

    Ok(())
}
