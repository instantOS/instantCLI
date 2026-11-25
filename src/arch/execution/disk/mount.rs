use super::{filesystem, home};
use crate::arch::engine::{
    BootMode, DualBootPartitions, EspNeedsFormat, FilesystemPlan, HomePartition, InstallContext,
    ManualPartitions,
};
use crate::arch::execution::CommandRunner;
use crate::common::blockdev::Filesystem;
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
    let kept_home_filesystem = manual_partitions
        .map(|partitions| home::preflight(partitions, executor))
        .transpose()?
        .flatten();

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
        super::activate_swap(&swap_path, executor)?;
    }

    if let Some(home) = manual_partitions.and_then(|parts| parts.home.as_ref()) {
        mount_home(home, kept_home_filesystem.as_ref(), executor)?;
    }

    Ok(())
}

fn mount_home(
    home: &HomePartition,
    kept_filesystem: Option<&Filesystem>,
    executor: &dyn CommandRunner,
) -> Result<()> {
    let path = home.path().as_str();
    let filesystem_type = match home {
        HomePartition::Format(_) => {
            println!("Formatting Home partition: {path}");
            filesystem::wipe_signatures(path, executor)?;
            executor.run(Command::new("mkfs.ext4").args(["-F", path]))?;
            "ext4"
        }
        HomePartition::Keep(_) => {
            println!("Keeping Home partition: {path}");
            kept_filesystem.map_or("auto", Filesystem::as_str)
        }
    };
    println!("Mounting Home partition: {path}");
    executor.run(Command::new("mount").args(["--mkdir", "-t", filesystem_type, path, "/mnt/home"]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::execution::mock::MockRunner;

    #[test]
    fn kept_home_is_mounted_without_formatting() {
        let runner = MockRunner::new();
        let home = HomePartition::from_answer("keep:/dev/home")
            .unwrap()
            .unwrap();
        mount_home(&home, Some(&Filesystem::parse("xfs")), &runner).unwrap();

        assert_eq!(
            runner.command_log(),
            ["mount --mkdir -t xfs /dev/home /mnt/home"]
        );
    }

    #[test]
    fn legacy_home_path_formats_ext4_before_mounting() {
        let runner = MockRunner::new();
        let home = HomePartition::from_answer("/dev/home").unwrap().unwrap();
        mount_home(&home, None, &runner).unwrap();

        assert_eq!(
            runner.command_log(),
            [
                "wipefs -a /dev/home",
                "mkfs.ext4 -F /dev/home",
                "mount --mkdir -t ext4 /dev/home /mnt/home",
            ]
        );
    }
}
