mod automatic;
mod dualboot;
mod encryption;
mod filesystem;
mod home;
mod partitions;
mod probe;
mod target;
mod util;

use std::fs;
use std::process::Command;

use anyhow::Result;

use super::CommandRunner;
use crate::arch::engine::{BootMode, InstallContext, InstallPlan, StoragePlan};

pub(crate) use home::validate_kept_home_devices;
use target::MountLocation;
pub(super) use target::PreparedTarget;
pub use util::get_part_path;

fn activate_swap(path: &str, executor: &dyn CommandRunner) -> Result<()> {
    if !executor.dry_run() {
        let device = fs::canonicalize(path)?;
        if fs::read_to_string("/proc/swaps")?
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .any(|active| fs::canonicalize(active).is_ok_and(|active| active == device))
        {
            return Ok(());
        }
        crate::arch::bootstrap::record(crate::arch::bootstrap::Resource::Swap(path.into()))?;
    }
    executor.run(Command::new("swapon").arg(path))
}

pub(super) fn check_target_disk(plan: &InstallPlan) -> Result<()> {
    let storage = plan.storage();
    let disk = storage.disk().as_str();
    crate::arch::disks::ensure_not_running_disk(disk)?;
    if let Some(conflict) = plan.environment().target() {
        anyhow::bail!(
            "{}",
            crate::arch::host::running_disk_message(disk, conflict)
        );
    }
    Ok(())
}

pub(super) fn check_target_for_preparation(
    plan: &InstallPlan,
    executor: &dyn CommandRunner,
) -> Result<()> {
    check_target_disk(plan)?;
    if plan.storage().encryption().is_some() {
        crate::arch::bootstrap::ensure_encryption_names_available()?;
    }
    if !executor.dry_run() {
        target::check_empty_target(executor)?;
    }
    Ok(())
}

pub(super) fn prepare_disk(
    plan: &InstallPlan,
    context: &InstallContext,
    executor: &dyn CommandRunner,
) -> Result<PreparedTarget> {
    let storage = plan.storage();
    let disk = storage.disk().as_str();
    if !executor.dry_run() {
        // Dual-boot resize has its own mount requirements, including online
        // shrink for btrfs. New Linux partitions are created in free space.
        if !matches!(storage, StoragePlan::DualBoot { .. }) {
            crate::arch::disks::prepare_disk(disk)?;
        }
    }

    println!("Preparing disk: {disk}");
    let boot_mode = plan.boot_mode();
    let uefi = matches!(boot_mode, BootMode::UEFI64 | BootMode::UEFI32);
    let ram = probe::get_total_ram_gb().unwrap_or(4);
    let swap_size = ram.max(4);
    println!("Detected RAM: {ram} GiB, setting Swap: {swap_size} GiB");

    let mut target = match storage {
        StoragePlan::DualBoot {
            filesystem, target, ..
        } => dualboot::prepare_dualboot_disk(
            context,
            *filesystem,
            target,
            boot_mode,
            executor,
            disk,
            swap_size,
        )?,
        StoragePlan::Manual {
            filesystem,
            partitions,
            ..
        } => partitions::prepare_manual(partitions, *filesystem, boot_mode, executor)?,
        StoragePlan::Automatic {
            filesystem,
            encryption: Some(encryption),
            ..
        } => {
            if uefi {
                encryption::partition_uefi_luks(disk, executor)?;
            } else {
                encryption::partition_bios_luks(disk, executor)?;
            }
            encryption::format_luks(*filesystem, encryption, disk, executor, uefi, swap_size)?;
            let mut target = PreparedTarget::new(
                "/dev/instantOS/root".into(),
                *filesystem,
                true,
                Some("/dev/instantOS/swap".into()),
                Some(get_part_path(disk, 2)),
            );
            target.add_mount(
                MountLocation::Boot,
                get_part_path(disk, 1),
                if uefi { "vfat" } else { "ext4" },
            );
            target
        }
        StoragePlan::Automatic {
            filesystem,
            encryption: None,
            ..
        } => {
            let (root, swap) = if uefi {
                automatic::partition_uefi(disk, executor, swap_size)?;
                automatic::format_uefi(*filesystem, disk, executor)?;
                (3, 2)
            } else {
                automatic::partition_bios(disk, executor, swap_size)?;
                automatic::format_bios(*filesystem, disk, executor)?;
                (2, 1)
            };
            let mut target = PreparedTarget::new(
                get_part_path(disk, root),
                *filesystem,
                true,
                Some(get_part_path(disk, swap)),
                None,
            );
            if uefi {
                target.add_mount(MountLocation::Boot, get_part_path(disk, 1), "vfat");
            }
            target
        }
    };
    target.prepare_root_subvolumes(storage.filesystem(), executor)?;
    if !executor.dry_run() {
        target.identify(executor)?;
    }
    Ok(target)
}
