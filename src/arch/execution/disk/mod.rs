mod automatic;
mod dualboot;
mod encryption;
mod filesystem;
mod mount;
mod probe;
mod util;

use super::CommandRunner;
use crate::arch::engine::{BootMode, InstallContext, InstallPlan, StoragePlan};
use anyhow::Result;

pub use util::get_part_path;

pub fn prepare_disk(
    plan: &InstallPlan,
    context: &InstallContext,
    executor: &dyn CommandRunner,
) -> Result<()> {
    let disk_path = plan.storage.disk().as_str();

    println!("Preparing disk: {}", disk_path);

    let boot_mode = plan.boot_mode();

    let ram_size_gb = probe::get_total_ram_gb().unwrap_or(4);
    let swap_size_gb = std::cmp::max(4, ram_size_gb);
    println!(
        "Detected RAM: {} GiB, setting Swap: {} GiB",
        ram_size_gb, swap_size_gb
    );

    // Dispatch on the structural storage plan. There is deliberately no
    // default: adding a storage strategy requires handling it here, and an
    // incomplete or unrecognized wizard answer cannot reach this layer.
    match &plan.storage {
        StoragePlan::DualBoot {
            filesystem, target, ..
        } => {
            dualboot::prepare_dualboot_disk(
                context,
                *filesystem,
                target,
                boot_mode,
                executor,
                disk_path,
                swap_size_gb,
            )?;
        }
        StoragePlan::Manual {
            filesystem,
            partitions,
            ..
        } => {
            mount::format_and_mount_partitions(
                context,
                *filesystem,
                Some(partitions),
                boot_mode,
                executor,
            )?;
        }
        StoragePlan::Automatic {
            filesystem,
            encryption,
            ..
        } => match (boot_mode, encryption.as_ref()) {
            (BootMode::UEFI64 | BootMode::UEFI32, None) => {
                automatic::partition_uefi(disk_path, executor, swap_size_gb)?;
                automatic::format_uefi(*filesystem, disk_path, executor)?;
                automatic::mount_uefi(*filesystem, disk_path, executor)?;
            }
            (BootMode::BIOS, None) => {
                automatic::partition_bios(disk_path, executor, swap_size_gb)?;
                automatic::format_bios(*filesystem, disk_path, executor)?;
                automatic::mount_bios(*filesystem, disk_path, executor)?;
            }
            (BootMode::UEFI64 | BootMode::UEFI32, Some(encryption)) => {
                encryption::partition_uefi_luks(disk_path, executor)?;
                encryption::format_luks(
                    *filesystem,
                    encryption,
                    disk_path,
                    executor,
                    true,
                    swap_size_gb,
                )?;
                encryption::mount_luks(*filesystem, boot_mode, executor, disk_path)?;
            }
            (BootMode::BIOS, Some(encryption)) => {
                encryption::partition_bios_luks(disk_path, executor)?;
                encryption::format_luks(
                    *filesystem,
                    encryption,
                    disk_path,
                    executor,
                    false,
                    swap_size_gb,
                )?;
                encryption::mount_luks(*filesystem, boot_mode, executor, disk_path)?;
            }
        },
    }

    Ok(())
}
