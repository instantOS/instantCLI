use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::{encryption, filesystem};
use crate::arch::engine::{EncryptionPlan, FilesystemPlan};
use crate::arch::execution::{CommandRunner, paths};

/// The filesystems created or selected by disk preparation. Their UUIDs guard
/// against a device path being reused for another disk between runs.
#[derive(Debug, Serialize, Deserialize)]
pub(in crate::arch::execution) struct PreparedTarget {
    mounts: Vec<TargetMount>,
    swap: Option<Device>,
    luks: Option<Device>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Device {
    path: String,
    uuid: String,
}

impl Device {
    fn new(path: String) -> Self {
        Self {
            path,
            uuid: String::new(),
        }
    }

    fn read_uuid(&self, executor: &dyn CommandRunner) -> Result<String> {
        let output = executor
            .run_with_output(
                Command::new("blkid").args(["-p", "-s", "UUID", "-o", "value", "--", &self.path]),
            )
            .with_context(|| format!("Could not read the filesystem UUID of {}", self.path))?
            .context("Missing UUID probe output")?;
        let uuid = String::from_utf8(output.stdout)?.trim().to_owned();
        ensure!(!uuid.is_empty(), "{} has no UUID", self.path);
        Ok(uuid)
    }

    fn verify(&self, executor: &dyn CommandRunner) -> Result<()> {
        ensure!(
            self.read_uuid(executor)? == self.uuid,
            "The UUID of {} changed since disk preparation. Refusing to use it for this installation.",
            self.path
        );
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TargetMount {
    device: Device,
    location: MountLocation,
    filesystem: String,
    subvolume: Option<String>,
    options: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MountLocation {
    Root,
    Boot,
    Efi,
    Home,
}

impl MountLocation {
    fn path(self) -> &'static str {
        match self {
            Self::Root => paths::CHROOT_MOUNT,
            Self::Boot => "/mnt/boot",
            Self::Efi => "/mnt/boot/efi",
            Self::Home => "/mnt/home",
        }
    }
}

impl PreparedTarget {
    pub(super) fn new(
        root: String,
        filesystem: FilesystemPlan,
        home_subvolume: bool,
        swap: Option<String>,
        luks: Option<String>,
    ) -> Self {
        use crate::arch::config::{BTRFS_HOME_SUBVOLUME, BTRFS_ROOT_SUBVOLUME};

        let mut target = Self {
            mounts: Vec::new(),
            swap: swap.map(Device::new),
            luks: luks.map(Device::new),
        };
        match filesystem {
            FilesystemPlan::Ext4 => target.add_mount(MountLocation::Root, root, "ext4"),
            FilesystemPlan::Btrfs { compression } => {
                let mut add_subvolume = |location, subvolume: &'static str| {
                    let mut options = format!("subvol={subvolume},noatime");
                    if let Some(compression) = compression.mount_option() {
                        options.push(',');
                        options.push_str(compression);
                    }
                    target.mounts.push(TargetMount {
                        device: Device::new(root.clone()),
                        location,
                        filesystem: "btrfs".into(),
                        subvolume: Some(format!("/{subvolume}")),
                        options: Some(options),
                    });
                };
                add_subvolume(MountLocation::Root, BTRFS_ROOT_SUBVOLUME);
                if home_subvolume {
                    add_subvolume(MountLocation::Home, BTRFS_HOME_SUBVOLUME);
                }
            }
        }
        target
    }

    pub(super) fn add_mount(&mut self, location: MountLocation, device: String, filesystem: &str) {
        self.mounts.push(TargetMount {
            device: Device::new(device),
            location,
            filesystem: filesystem.into(),
            subvolume: None,
            options: None,
        });
    }

    fn devices(&self) -> impl Iterator<Item = &Device> {
        self.mounts
            .iter()
            .map(|mount| &mount.device)
            .chain(&self.swap)
    }

    pub(super) fn prepare_root_subvolumes(
        &self,
        filesystem: FilesystemPlan,
        executor: &dyn CommandRunner,
    ) -> Result<()> {
        let root = self.mounts.first().context("Missing target root")?;
        let home_subvolume = self
            .mounts
            .iter()
            .any(|mount| mount.location == MountLocation::Home && mount.subvolume.is_some());
        filesystem::create_root_subvolumes(filesystem, &root.device.path, home_subvolume, executor)
    }

    pub(super) fn identify(&mut self, executor: &dyn CommandRunner) -> Result<()> {
        for device in self
            .mounts
            .iter_mut()
            .map(|mount| &mut mount.device)
            .chain(&mut self.swap)
            .chain(&mut self.luks)
        {
            device.uuid = device.read_uuid(executor)?;
        }
        Ok(())
    }

    pub(in crate::arch::execution) fn ensure_mounted(
        &self,
        encryption: Option<&EncryptionPlan>,
        executor: &dyn CommandRunner,
    ) -> Result<()> {
        let mounted = if executor.dry_run() {
            Vec::new()
        } else {
            ensure!(
                self.mounts
                    .first()
                    .is_some_and(|mount| mount.location == MountLocation::Root),
                "The saved installation state has no root filesystem"
            );
            let mounted = mounted_filesystems(executor)?;
            check_mounts(&self.mounts, &mounted)?;
            if let Some(luks) = &self.luks {
                luks.verify(executor)?;
                encryption::open_prepared(
                    &luks.path,
                    encryption.context("Missing encryption plan")?,
                    executor,
                )?;
            }
            for device in self.devices() {
                device.verify(executor)?;
            }
            mounted
        };

        for mount in &self.mounts {
            if !mounted
                .iter()
                .any(|entry| entry.target == mount.location.path())
            {
                if !executor.dry_run() {
                    check_mount_path(mount.location.path())?;
                }
                let mut command = Command::new("mount");
                if mount.location != MountLocation::Root {
                    command.arg("--mkdir");
                }
                command.args(["-t", &mount.filesystem]);
                if let Some(options) = &mount.options {
                    command.args(["-o", options]);
                }
                executor.run(command.args(["--", &mount.device.path, mount.location.path()]))?;
            }
        }

        if !executor.dry_run() {
            let mounted = mounted_filesystems(executor)?;
            check_mounts(&self.mounts, &mounted)?;
            for mount in &self.mounts {
                ensure!(
                    mounted
                        .iter()
                        .any(|entry| entry.target == mount.location.path()),
                    "{} is not mounted",
                    mount.location.path()
                );
            }
        }

        if let Some(swap) = &self.swap {
            super::activate_swap(&swap.path, executor)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct MountedFilesystem {
    target: String,
    source: String,
    fsroot: String,
    options: String,
}

fn mounted_filesystems(executor: &dyn CommandRunner) -> Result<Vec<MountedFilesystem>> {
    #[derive(Deserialize)]
    struct MountTable {
        filesystems: Vec<MountedFilesystem>,
    }
    let output = executor
        .run_with_output(Command::new("findmnt").args([
            "--kernel",
            "--list",
            "--json",
            "--nofsroot",
            "--output",
            "TARGET,SOURCE,FSROOT,OPTIONS",
        ]))?
        .context("Missing mount table")?;
    Ok(serde_json::from_slice::<MountTable>(&output.stdout)?.filesystems)
}

fn under_target(path: &str) -> bool {
    Path::new(path).starts_with(paths::CHROOT_MOUNT)
}

fn check_mount_path(path: &str) -> Result<()> {
    for directory in Path::new(path)
        .ancestors()
        .take_while(|path| path.starts_with(paths::CHROOT_MOUNT))
    {
        match fs::symlink_metadata(directory) {
            Ok(metadata) => ensure!(
                metadata.is_dir(),
                "{} is not a regular mount directory",
                directory.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("Inspecting {}", directory.display()));
            }
        }
    }
    Ok(())
}

fn check_mounts(expected: &[TargetMount], mounted: &[MountedFilesystem]) -> Result<()> {
    let root_mounted = mounted
        .iter()
        .any(|entry| entry.target == paths::CHROOT_MOUNT);
    for entry in mounted.iter().filter(|entry| under_target(&entry.target)) {
        // An offline chroot step can leave its bundle bind mounted for the
        // next step. It must still refer to the live environment's bundle.
        if !expected.is_empty()
            && root_mounted
            && entry.target
                == paths::chroot_path(crate::arch::offline::BUNDLE_MOUNT).to_string_lossy()
            && mounted.iter().any(|live| {
                live.target == crate::arch::offline::BUNDLE_MOUNT
                    && live.source == entry.source
                    && live.fsroot == entry.fsroot
            })
        {
            continue;
        }
        let mount = expected
            .iter()
            .find(|mount| mount.location.path() == entry.target)
            .with_context(|| {
                format!(
                    "Unexpected mount at {}. Unmount it before continuing.",
                    entry.target
                )
            })?;
        ensure!(
            root_mounted
                && mounted
                    .iter()
                    .filter(|other| other.target == entry.target)
                    .count()
                    == 1,
            "Unexpected mount layout at {}. Unmount it before continuing.",
            entry.target
        );
        ensure!(
            fs::canonicalize(&entry.source)? == fs::canonicalize(&mount.device.path)?
                && entry.fsroot == mount.subvolume.as_deref().unwrap_or("/")
                && entry.options.split(',').any(|option| option == "rw"),
            "{} is not mounted from the expected writable filesystem ({}). Unmount it before continuing.",
            entry.target,
            mount.device.path
        );
    }
    Ok(())
}

pub(super) fn check_empty_target(executor: &dyn CommandRunner) -> Result<()> {
    check_mount_path(paths::CHROOT_MOUNT)?;
    check_mounts(&[], &mounted_filesystems(executor)?)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    use super::*;
    use crate::arch::execution::mock::MockRunner;

    struct MountRunner {
        log: MockRunner,
        root_source: Option<&'static str>,
        mounted: Cell<bool>,
    }

    impl CommandRunner for MountRunner {
        fn dry_run(&self) -> bool {
            false
        }

        fn run(&self, command: &mut Command) -> Result<()> {
            ensure!(
                command.get_program() == "mount",
                "only mount should run during resume"
            );
            self.mounted.set(true);
            self.log.run(command)
        }

        fn run_with_input(&self, _: &mut Command, _: &str) -> Result<()> {
            anyhow::bail!("resume must not execute disk preparation commands")
        }

        fn run_with_output(&self, command: &mut Command) -> Result<Option<Output>> {
            let stdout =
                match command.get_program().to_str() {
                    Some("findmnt") => {
                        let source = if self.mounted.get() {
                            Some("/dev/null")
                        } else {
                            self.root_source
                        };
                        let filesystems = source.into_iter().map(|source| serde_json::json!({
                        "target": "/mnt", "source": source, "fsroot": "/", "options": "rw",
                    })).collect::<Vec<_>>();
                        serde_json::to_vec(&serde_json::json!({ "filesystems": filesystems }))?
                    }
                    Some("blkid") => b"prepared-uuid\n".to_vec(),
                    _ => anyhow::bail!("unexpected command: {command:?}"),
                };
            Ok(Some(Output {
                status: ExitStatus::from_raw(0),
                stdout,
                stderr: Vec::new(),
            }))
        }

        fn log(&self, _: &str) {}
    }

    #[test]
    fn remounts_prepared_storage_without_formatting() -> Result<()> {
        let mut target =
            PreparedTarget::new("/dev/null".into(), FilesystemPlan::Ext4, false, None, None);
        let runner = MountRunner {
            log: MockRunner::new(),
            root_source: None,
            mounted: Cell::new(false),
        };
        target.identify(&runner)?;
        target.ensure_mounted(None, &runner)?;
        target.ensure_mounted(None, &runner)?;
        assert_eq!(
            runner.log.command_log(),
            ["mount -t ext4 -- /dev/null /mnt"]
        );
        Ok(())
    }

    #[test]
    fn refuses_a_different_filesystem_at_the_target() -> Result<()> {
        let mut target =
            PreparedTarget::new("/dev/null".into(), FilesystemPlan::Ext4, false, None, None);
        let runner = MountRunner {
            log: MockRunner::new(),
            root_source: Some("/dev/zero"),
            mounted: Cell::new(false),
        };
        target.identify(&runner)?;
        let error = target.ensure_mounted(None, &runner).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("/mnt is not mounted from the expected")
        );
        assert!(runner.log.command_log().is_empty());
        Ok(())
    }

    #[test]
    fn refuses_the_wrong_btrfs_subvolume() {
        let target = PreparedTarget::new(
            "/dev/null".into(),
            FilesystemPlan::Btrfs {
                compression: crate::arch::config::BtrfsCompression::Zstd,
            },
            true,
            None,
            None,
        );
        let mounted = [MountedFilesystem {
            target: "/mnt".into(),
            source: "/dev/null".into(),
            fsroot: "/@home".into(),
            options: "rw".into(),
        }];
        assert!(check_mounts(&target.mounts, &mounted).is_err());
    }
}
