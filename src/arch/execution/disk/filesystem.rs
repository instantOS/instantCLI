use crate::arch::config::{BtrfsCompression, RootFilesystem};
use crate::arch::engine::InstallContext;
use crate::arch::execution::CommandRunner;
use anyhow::Result;
use std::process::Command;

pub fn format_root(
    context: &InstallContext,
    device: &str,
    executor: &dyn CommandRunner,
) -> Result<()> {
    match RootFilesystem::from_context(context) {
        RootFilesystem::Btrfs => {
            executor.run(Command::new("mkfs.btrfs").args(["-f", device]))?;
        }
        RootFilesystem::Ext4 => {
            executor.run(Command::new("mkfs.ext4").args(["-F", device]))?;
        }
    }
    Ok(())
}

pub fn mount_root(
    context: &InstallContext,
    device: &str,
    create_home_subvolume: bool,
    executor: &dyn CommandRunner,
) -> Result<()> {
    if !RootFilesystem::from_context(context).is_btrfs() {
        executor.run(Command::new("mount").args([device, "/mnt"]))?;
        return Ok(());
    }

    // Create subvolumes from the top-level btrfs tree, then remount the root
    // subvolume. Keeping @home separate allows snapshots of @ without rolling
    // back user data.
    executor.run(Command::new("mount").args([device, "/mnt"]))?;
    executor.run(Command::new("btrfs").args(["subvolume", "create", "/mnt/@"]))?;
    if create_home_subvolume {
        executor.run(Command::new("btrfs").args(["subvolume", "create", "/mnt/@home"]))?;
    }
    executor.run(Command::new("umount").arg("/mnt"))?;

    let compression = BtrfsCompression::from_context(context);
    let mount_options = |subvolume| {
        let mut options = vec![subvolume, "noatime"];
        if let Some(option) = compression.mount_option() {
            options.push(option);
        }
        options.join(",")
    };

    let options = mount_options("subvol=@");
    executor.run(Command::new("mount").args(["-o", &options, device, "/mnt"]))?;

    if create_home_subvolume {
        let home_options = mount_options("subvol=@home");
        executor.run(Command::new("mount").args([
            "--mkdir",
            "-o",
            &home_options,
            device,
            "/mnt/home",
        ]))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::engine::QuestionId;
    use crate::arch::execution::mock::MockRunner;

    fn context(filesystem: &str, compression: &str) -> InstallContext {
        let mut context = InstallContext::new();
        context
            .answers
            .insert(QuestionId::RootFilesystem, filesystem.into());
        context
            .answers
            .insert(QuestionId::BtrfsCompression, compression.into());
        context
    }

    #[test]
    fn formats_and_mounts_btrfs_subvolumes() {
        let runner = MockRunner::new();
        let context = context("btrfs", "zstd");

        format_root(&context, "/dev/root", &runner).unwrap();
        mount_root(&context, "/dev/root", true, &runner).unwrap();

        let log = runner.command_log();
        assert!(log.iter().any(|line| line == "mkfs.btrfs -f /dev/root"));
        assert!(
            log.iter()
                .any(|line| line.contains("subvolume create /mnt/@home"))
        );
        assert!(log.iter().any(|line| {
            line.contains("subvol=@,noatime,compress=zstd") && line.ends_with("/dev/root /mnt")
        }));
    }

    #[test]
    fn ext4_does_not_create_subvolumes() {
        let runner = MockRunner::new();
        let context = context("ext4", "zstd");

        format_root(&context, "/dev/root", &runner).unwrap();
        mount_root(&context, "/dev/root", true, &runner).unwrap();

        assert_eq!(
            runner.command_log(),
            vec!["mkfs.ext4 -F /dev/root", "mount /dev/root /mnt"]
        );
    }
}
