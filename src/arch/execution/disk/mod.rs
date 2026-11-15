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

fn activate_swap(path: &str, executor: &dyn CommandRunner) -> Result<()> {
    if !executor.dry_run() {
        crate::arch::bootstrap::record(crate::arch::bootstrap::Resource::Swap(path.into()))?;
    }
    executor.run(std::process::Command::new("swapon").arg(path))
}

pub fn prepare_disk(
    plan: &InstallPlan,
    context: &InstallContext,
    executor: &dyn CommandRunner,
) -> Result<()> {
    let disk = plan.storage.disk();
    // Recheck source protection at the mutation boundary, including plans
    // imported without the wizard and device aliases.
    crate::arch::disks::ensure_not_running_disk(disk.as_str())?;
    if plan.storage.encryption().is_some() {
        crate::arch::bootstrap::ensure_encryption_names_available()?;
    }

    // Last line of defence before partitioning. The wizard's disk question
    // refuses the running disk, and `crate::arch::disks::prepare_disk` refuses
    // to unmount it — but this step is the one that writes a partition table,
    // so a hand-authored configuration that names the running disk must still
    // stop here rather than repartition the machine the installer is executing
    // from.
    //
    // Retain the plan's original conflict as well as the fresh check above:
    // a disk protected when the plan was built must stay protected.
    if let Some(conflict) = plan.environment.target() {
        anyhow::bail!(
            "{}",
            crate::arch::host::running_disk_message(disk.as_str(), conflict)
        );
    }

    let disk_path = disk.as_str();
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
