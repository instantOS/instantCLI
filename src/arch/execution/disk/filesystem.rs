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

pub fn create_root_subvolumes(
    filesystem: FilesystemPlan,
    device: &str,
    create_home_subvolume: bool,
    executor: &dyn CommandRunner,
) -> Result<()> {
    if !matches!(filesystem, FilesystemPlan::Btrfs { .. }) {
        return Ok(());
    }

    // Create subvolumes on the top-level tree before mounting the target.
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
    fn formats_and_creates_btrfs_subvolumes() {
        let runner = MockRunner::new();
        let filesystem = FilesystemPlan::Btrfs {
            compression: BtrfsCompression::Zstd,
        };

        format_root(filesystem, "/dev/root", &runner).unwrap();
        create_root_subvolumes(filesystem, "/dev/root", true, &runner).unwrap();

        let log = runner.command_log();
        assert!(log.iter().any(|line| line == "wipefs -a /dev/root"));
        assert!(log.iter().any(|line| line == "mkfs.btrfs -f /dev/root"));
        assert!(
            log.iter()
                .any(|line| line.contains("subvolume create /mnt/@home"))
        );
        assert_eq!(log.last().unwrap(), "umount /mnt");
    }

    #[test]
    fn ext4_does_not_create_subvolumes() {
        let runner = MockRunner::new();
        format_root(FilesystemPlan::Ext4, "/dev/root", &runner).unwrap();
        create_root_subvolumes(FilesystemPlan::Ext4, "/dev/root", true, &runner).unwrap();

        assert_eq!(
            runner.command_log(),
            vec![
                "wipefs -a /dev/root",
                "mkfs.ext4 -F /dev/root",
                "udevadm settle",
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

        let error = create_root_subvolumes(filesystem, "/dev/root", true, &runner).unwrap_err();

        assert!(error.to_string().contains("simulated subvolume failure"));
        assert_eq!(runner.inner.command_log().last().unwrap(), "umount /mnt");
    }
}
