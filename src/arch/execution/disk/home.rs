use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::arch::engine::{HomePartition, ManualPartitions};
use crate::arch::execution::CommandRunner;
use crate::common::blockdev::Filesystem;

pub(crate) fn validate_kept_home_devices(partitions: &ManualPartitions) -> Result<()> {
    let Some(HomePartition::Keep(home)) = &partitions.home else {
        return Ok(());
    };
    let home_id = block_device_id(home.as_str())?;
    for partition in [&partitions.root, &partitions.boot]
        .into_iter()
        .chain(partitions.swap.iter())
    {
        ensure!(
            block_device_id(partition.as_str())? != home_id,
            "Cannot keep {} as /home because {} names the same device and will be formatted",
            home.as_str(),
            partition.as_str(),
        );
    }
    Ok(())
}

fn block_device_id(path: &str) -> Result<u64> {
    let metadata = fs::metadata(path).with_context(|| format!("Cannot inspect {path}"))?;
    ensure!(
        metadata.file_type().is_block_device(),
        "{path} is not a block device"
    );
    Ok(metadata.rdev())
}

pub(super) fn preflight(
    partitions: &ManualPartitions,
    executor: &dyn CommandRunner,
) -> Result<Option<Filesystem>> {
    let Some(HomePartition::Keep(home)) = &partitions.home else {
        return Ok(None);
    };
    if executor.dry_run() {
        return Ok(None);
    }

    validate_kept_home_devices(partitions)?;
    let path = home.as_str();
    let output = executor
        .run_with_output(Command::new("blkid").args(["-p", "-s", "TYPE", "-o", "value", path]))
        .with_context(|| format!("Could not identify the filesystem to keep on {path}"))?
        .context("Missing output from blkid")?;
    let filesystem = Filesystem::parse(std::str::from_utf8(&output.stdout)?);
    ensure!(
        !filesystem.as_str().is_empty(),
        "Cannot keep {path} as /home: no filesystem was detected"
    );
    ensure!(
        filesystem.supports_home_reuse(),
        "Cannot keep {path} as /home: {filesystem} is unsupported. Choose an ext2, ext3, ext4, or XFS partition."
    );

    // This directory may hold a mount. Only remove it nonrecursively.
    let mountpoint = tempfile::tempdir()
        .context("Creating a mount point for the /home check")?
        .keep();
    let mut options = vec!["ro", "nodev", "nosuid", "noexec"];
    options.extend(filesystem.read_only_options());
    let mount_result = executor
        .run(
            Command::new("mount")
                .args(["-t", filesystem.as_str(), "-o", &options.join(","), path])
                .arg(&mountpoint),
        )
        .with_context(|| format!("Could not mount {path} read-only to check /home"));
    if let Err(error) = mount_result {
        return match fs::remove_dir(&mountpoint) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(error.context(format!(
                "Could not remove {}: {cleanup}",
                mountpoint.display()
            ))),
        };
    }

    executor
        .run(Command::new("umount").arg(&mountpoint))
        .with_context(|| format!("Could not unmount {path} from {}", mountpoint.display()))?;
    fs::remove_dir(&mountpoint).context("Removing the temporary /home mount point")?;
    Ok(Some(filesystem))
}
