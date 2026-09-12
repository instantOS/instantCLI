mod automatic;
mod dualboot;
mod encryption;
mod filesystem;
mod mount;
mod probe;
mod util;

use super::CommandRunner;
use crate::arch::engine::{BootMode, InstallContext, StepId};
use anyhow::{Context, Result};

pub use util::get_part_path;

pub fn prepare_disk(context: &InstallContext, executor: &dyn CommandRunner) -> Result<()> {
    let disk_path = context
        .get_answer(&StepId::Disk)
        .context("No disk selected")?;

    println!("Preparing disk: {}", disk_path);

    let boot_mode = &context.system_info.boot_mode;

    let ram_size_gb = probe::get_total_ram_gb().unwrap_or(4);
    let swap_size_gb = std::cmp::max(4, ram_size_gb);
    println!(
        "Detected RAM: {} GiB, setting Swap: {} GiB",
        ram_size_gb, swap_size_gb
    );

    // Dispatch on the typed partitioning kind. There is deliberately no
    // default: an unanswered or unrecognized method must fail before any
    // partitioning runs, never fall through to the disk-erasing path.
    match context.partitioning_kind() {
        crate::arch::engine::PartitioningKind::DualBoot => {
            dualboot::prepare_dualboot_disk(context, executor, disk_path, swap_size_gb)?;
        }
        crate::arch::engine::PartitioningKind::Manual => {
            mount::format_and_mount_partitions(context, executor)?;
        }
        crate::arch::engine::PartitioningKind::Automatic => {
            let use_encryption = context.get_answer_bool(StepId::UseEncryption);

            match (boot_mode, use_encryption) {
                (BootMode::UEFI64 | BootMode::UEFI32, false) => {
                    automatic::partition_uefi(disk_path, executor, swap_size_gb)?;
                    automatic::format_uefi(context, disk_path, executor)?;
                    automatic::mount_uefi(context, disk_path, executor)?;
                }
                (BootMode::BIOS, false) => {
                    automatic::partition_bios(disk_path, executor, swap_size_gb)?;
                    automatic::format_bios(context, disk_path, executor)?;
                    automatic::mount_bios(context, disk_path, executor)?;
                }
                (BootMode::UEFI64 | BootMode::UEFI32, true) => {
                    encryption::partition_uefi_luks(disk_path, executor)?;
                    encryption::format_luks(context, disk_path, executor, true, swap_size_gb)?;
                    encryption::mount_luks(context, executor, disk_path)?;
                }
                (BootMode::BIOS, true) => {
                    encryption::partition_bios_luks(disk_path, executor)?;
                    encryption::format_luks(context, disk_path, executor, false, swap_size_gb)?;
                    encryption::mount_luks(context, executor, disk_path)?;
                }
            }
        }
        kind => anyhow::bail!(
            "Partitioning method was not answered or is unrecognized (kind: {kind}); \
             refusing to partition {} automatically",
            disk_path
        ),
    }

    Ok(())
}
