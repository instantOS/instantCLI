use crate::arch::config::{BTRFS_HOME_SUBVOLUME, BTRFS_ROOT_SUBVOLUME};
use crate::arch::engine::FilesystemPlan;
use crate::arch::execution::CommandRunner;
use anyhow::Result;
use std::process::Command;

/// Remove stale filesystem signatures from a device before formatting it.
///
/// Re-running the installer over a previous (possibly failed) install leaves
/// old swap/btrfs/ext4 signatures behind. mkfs force flags (`-F`, `-f`) plow
/// through them but do not remove every trace (e.g. btrfs backup superblocks),
/// and leftovers can confuse filesystem auto-detection when mounting. `wipefs`
/// is a no-op on clean devices and fails safely if the device is in use.
pub fn wipe_signatures(device: &str, executor: &dyn CommandRunner) -> Result<()> {
    executor.run(Command::new("wipefs").args(["-a", device]))
}

pub fn format_root(
    filesystem: FilesystemPlan,
    device: &str,
    executor: &dyn CommandRunner,
) -> Result<()> {
    wipe_signatures(device, executor)?;

    match filesystem {
        FilesystemPlan::Btrfs { .. } => {
            executor.run(Command::new("mkfs.btrfs").args(["-f", device]))?;
        }
        FilesystemPlan::Ext4 => {
            executor.run(Command::new("mkfs.ext4").args(["-F", device]))?;
        }
    }

    // Let udev process the change events from mkfs before anything probes or
    // mounts the freshly formatted device.
    if !executor.dry_run() {
        executor.run(Command::new("udevadm").arg("settle"))?;
    }

    Ok(())
}

pub fn mount_root(
    filesystem: FilesystemPlan,
    device: &str,
    create_home_subvolume: bool,
    executor: &dyn CommandRunner,
) -> Result<()> {
    let FilesystemPlan::Btrfs { compression } = filesystem else {
        // Explicit fstype: auto-detection can be thrown off by stale
        // signatures on re-partitioned disks.
        executor.run(Command::new("mount").args(["-t", "ext4", device, "/mnt"]))?;
        return Ok(());
    };

    // Create subvolumes from the top-level btrfs tree, then remount the root
    // subvolume. Keeping @home separate allows snapshots of @ without rolling
    // back user data.
    executor.run(Command::new("mount").args(["-t", "btrfs", device, "/mnt"]))?;
    let create_result = (|| -> Result<()> {
        let root_path = format!("/mnt/{BTRFS_ROOT_SUBVOLUME}");
        executor.run(Command::new("btrfs").args(["subvolume", "create", &root_path]))?;
        if create_home_subvolume {
            let home_path = format!("/mnt/{BTRFS_HOME_SUBVOLUME}");
            executor.run(Command::new("btrfs").args(["subvolume", "create", &home_path]))?;
        }
        Ok(())
    })();
    let unmount_result = executor.run(Command::new("umount").arg("/mnt"));

    if let Err(error) = create_result {
        // Preserve the subvolume error while still making a best-effort attempt
        // to leave /mnt clean for a same-session retry.
        let _ = unmount_result;
        return Err(error);
    }
    unmount_result?;

    let mount_options = |subvolume| {
        let mut options = vec![subvolume, "noatime"];
        if let Some(option) = compression.mount_option() {
            options.push(option);
        }
        options.join(",")
    };

    let root_subvolume = format!("subvol={BTRFS_ROOT_SUBVOLUME}");
    let options = mount_options(&root_subvolume);
    executor.run(Command::new("mount").args(["-t", "btrfs", "-o", &options, device, "/mnt"]))?;

    if create_home_subvolume {
        let home_subvolume = format!("subvol={BTRFS_HOME_SUBVOLUME}");
        let home_options = mount_options(&home_subvolume);
        executor.run(Command::new("mount").args([
            "--mkdir",
            "-t",
            "btrfs",
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
    use crate::arch::config::BtrfsCompression;
    use crate::arch::execution::mock::MockRunner;
    use std::process::Output;

    struct FailOnHomeSubvolume {
        inner: MockRunner,
    }

    impl CommandRunner for FailOnHomeSubvolume {
        fn dry_run(&self) -> bool {
            false
        }

        fn run(&self, command: &mut Command) -> Result<()> {
            let should_fail = command.get_args().any(|argument| argument == "/mnt/@home");
            self.inner.run(command)?;
            if should_fail {
                anyhow::bail!("simulated subvolume failure");
            }
            Ok(())
        }

        fn run_with_input(&self, command: &mut Command, input: &str) -> Result<()> {
            self.inner.run_with_input(command, input)
        }

        fn run_with_output(&self, command: &mut Command) -> Result<Option<Output>> {
            self.inner.run_with_output(command)
        }

        fn log(&self, message: &str) {
            self.inner.log(message);
        }
    }

    #[test]
    fn formats_and_mounts_btrfs_subvolumes() {
        let runner = MockRunner::new();
        let filesystem = FilesystemPlan::Btrfs {
            compression: BtrfsCompression::Zstd,
        };

        format_root(filesystem, "/dev/root", &runner).unwrap();
        mount_root(filesystem, "/dev/root", true, &runner).unwrap();

        let log = runner.command_log();
        assert!(log.iter().any(|line| line == "wipefs -a /dev/root"));
        assert!(log.iter().any(|line| line == "mkfs.btrfs -f /dev/root"));
        assert!(
            log.iter()
                .any(|line| line.contains("subvolume create /mnt/@home"))
        );
        assert!(log.iter().any(|line| {
            line.contains("-t btrfs")
                && line.contains("subvol=@,noatime,compress=zstd")
                && line.ends_with("/dev/root /mnt")
        }));
    }

    #[test]
    fn ext4_does_not_create_subvolumes() {
        let runner = MockRunner::new();
        format_root(FilesystemPlan::Ext4, "/dev/root", &runner).unwrap();
        mount_root(FilesystemPlan::Ext4, "/dev/root", true, &runner).unwrap();

        assert_eq!(
            runner.command_log(),
            vec![
                "wipefs -a /dev/root",
                "mkfs.ext4 -F /dev/root",
                "udevadm settle",
                "mount -t ext4 /dev/root /mnt"
            ]
        );
    }

    #[test]
    fn unmounts_top_level_after_subvolume_creation_failure() {
        let runner = FailOnHomeSubvolume {
            inner: MockRunner::new(),
        };
        let filesystem = FilesystemPlan::Btrfs {
            compression: BtrfsCompression::Zstd,
        };

        let error = mount_root(filesystem, "/dev/root", true, &runner).unwrap_err();

        assert!(error.to_string().contains("simulated subvolume failure"));
        assert_eq!(runner.inner.command_log().last().unwrap(), "umount /mnt");
    }
}
