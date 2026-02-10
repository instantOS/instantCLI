mod automatic;
mod dualboot;
mod encryption;
mod mount;
mod probe;
mod util;

use super::CommandExecutor;
use crate::arch::engine::{BootMode, InstallContext, QuestionId};
use anyhow::{Context, Result};

pub use util::get_part_path;

pub fn prepare_disk(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    let disk_path = context
        .get_answer(&QuestionId::Disk)
        .context("No disk selected")?;

    println!("Preparing disk: {}", disk_path);

    let boot_mode = &context.system_info.boot_mode;

    let ram_size_gb = probe::get_total_ram_gb().unwrap_or(4);
    let swap_size_gb = std::cmp::max(4, ram_size_gb);
    println!(
        "Detected RAM: {} GiB, setting Swap: {} GiB",
        ram_size_gb, swap_size_gb
    );

    let partitioning_method = context
        .get_answer(&QuestionId::PartitioningMethod)
        .map(|s| s.as_str())
        .unwrap_or("Automatic");

    if partitioning_method.contains("Dual Boot") {
        dualboot::prepare_dualboot_disk(context, executor, disk_path, swap_size_gb)?;
    } else if partitioning_method.contains("Manual") {
        mount::format_and_mount_partitions(context, executor)?;
    } else {
        let use_encryption = context.get_answer_bool(QuestionId::UseEncryption);

        match (boot_mode, use_encryption) {
            (BootMode::UEFI64 | BootMode::UEFI32, false) => {
                automatic::partition_uefi(disk_path, executor, swap_size_gb)?;
                automatic::format_uefi(disk_path, executor)?;
                automatic::mount_uefi(disk_path, executor)?;
            }
            (BootMode::BIOS, false) => {
                automatic::partition_bios(disk_path, executor, swap_size_gb)?;
                automatic::format_bios(disk_path, executor)?;
                automatic::mount_bios(disk_path, executor)?;
            }
            (BootMode::UEFI64 | BootMode::UEFI32, true) => {
                encryption::partition_uefi_luks(disk_path, executor)?;
                encryption::format_luks(context, disk_path, executor, true, swap_size_gb)?;
                encryption::mount_luks(executor, disk_path)?;
            }
            (BootMode::BIOS, true) => {
                encryption::partition_bios_luks(disk_path, executor)?;
                encryption::format_luks(context, disk_path, executor, false, swap_size_gb)?;
                encryption::mount_luks(executor, disk_path)?;
            }
        }
    }

    Ok(())
}
