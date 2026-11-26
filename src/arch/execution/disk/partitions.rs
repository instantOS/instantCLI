use std::process::Command;

use anyhow::Result;

use super::target::{MountLocation, PreparedTarget};
use super::{filesystem, home};
use crate::arch::engine::{BootMode, FilesystemPlan, HomePartition, ManualPartitions};
use crate::arch::execution::CommandRunner;
use crate::common::blockdev::Filesystem;

pub(super) fn prepare_manual(
    partitions: &ManualPartitions,
    filesystem_plan: FilesystemPlan,
    boot_mode: &BootMode,
    executor: &dyn CommandRunner,
) -> Result<PreparedTarget> {
    let kept_home = home::preflight(partitions, executor)?;
    let root = partitions.root.as_str();
    let boot = partitions.boot.as_str();
    let swap = partitions
        .swap
        .as_ref()
        .map(|path| path.as_str().to_owned());

    filesystem::format_root(filesystem_plan, root, executor)?;
    let boot_type = format_boot(boot, boot_mode, executor)?;
    if let Some(swap) = &swap {
        format_swap(swap, executor)?;
    }

    let mut target = PreparedTarget::new(
        root.into(),
        filesystem_plan,
        partitions.home.is_none(),
        swap,
        None,
    );
    target.add_mount(MountLocation::Boot, boot.into(), boot_type);
    if let Some(home) = &partitions.home {
        let filesystem = prepare_home(home, kept_home.as_ref(), executor)?;
        target.add_mount(MountLocation::Home, home.path().as_str().into(), filesystem);
    }
    Ok(target)
}

pub(super) fn format_boot(
    path: &str,
    boot_mode: &BootMode,
    executor: &dyn CommandRunner,
) -> Result<&'static str> {
    filesystem::wipe_signatures(path, executor)?;
    match boot_mode {
        BootMode::UEFI64 | BootMode::UEFI32 => {
            executor.run(Command::new("mkfs.fat").args(["-F32", path]))?;
            Ok("vfat")
        }
        BootMode::BIOS => {
            executor.run(Command::new("mkfs.ext4").args(["-F", path]))?;
            Ok("ext4")
        }
    }
}

pub(super) fn format_swap(path: &str, executor: &dyn CommandRunner) -> Result<()> {
    filesystem::wipe_signatures(path, executor)?;
    executor.run(Command::new("mkswap").arg(path))
}

fn prepare_home<'a>(
    home: &HomePartition,
    kept_filesystem: Option<&'a Filesystem>,
    executor: &dyn CommandRunner,
) -> Result<&'a str> {
    let path = home.path().as_str();
    match home {
        HomePartition::Format(_) => {
            println!("Formatting Home partition: {path}");
            filesystem::wipe_signatures(path, executor)?;
            executor.run(Command::new("mkfs.ext4").args(["-F", path]))?;
            Ok("ext4")
        }
        HomePartition::Keep(_) => {
            println!("Keeping Home partition: {path}");
            Ok(kept_filesystem.map_or("auto", Filesystem::as_str))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::execution::mock::MockRunner;

    #[test]
    fn kept_home_preserves_the_filesystem() {
        let runner = MockRunner::new();
        let home = HomePartition::from_answer("keep:/dev/home")
            .unwrap()
            .unwrap();
        assert_eq!(
            prepare_home(&home, Some(&Filesystem::parse("xfs")), &runner).unwrap(),
            "xfs"
        );
        assert!(runner.command_log().is_empty());
    }

    #[test]
    fn legacy_home_path_formats_ext4() {
        let runner = MockRunner::new();
        let home = HomePartition::from_answer("/dev/home").unwrap().unwrap();
        assert_eq!(prepare_home(&home, None, &runner).unwrap(), "ext4");
        assert_eq!(
            runner.command_log(),
            ["wipefs -a /dev/home", "mkfs.ext4 -F /dev/home"]
        );
    }
}
