//! Disk identities captured in the source mount namespace, before chrooting.
use std::collections::BTreeMap;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Topology {
    blockdevices: Vec<BlockDevice>,
}

#[derive(Debug, Deserialize)]
struct BlockDevice {
    name: String,
    #[serde(rename = "maj:min")]
    identity: String,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    #[serde(default)]
    children: Vec<BlockDevice>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct ProtectedDevices(BTreeMap<String, String>);

impl ProtectedDevices {
    pub fn capture() -> Result<Self> {
        let output = Command::new("lsblk")
            .args(["--json", "--paths", "--output", "NAME,MAJ:MIN,MOUNTPOINTS"])
            .output()
            .context("Could not inspect host disks")?;
        if !output.status.success() {
            bail!("Host disk inspection failed");
        }
        let topology: Topology = serde_json::from_slice(&output.stdout)?;
        let root = Command::new("findmnt")
            .args(["--noheadings", "--output", "MAJ:MIN", "--target", "/"])
            .output()?;
        if !root.status.success() {
            bail!("Could not identify the host root filesystem");
        }
        let mut devices = Self::from_topology(&topology);
        let identity = String::from_utf8(root.stdout)?.trim().to_owned();
        // lsblk cannot reliably attribute every member of a multi-device btrfs
        // filesystem. Resolve those members explicitly rather than trusting the
        // single device displayed by findmnt.
        let filesystems = Command::new("findmnt")
            .args(["-rn", "-t", "btrfs", "-o", "TARGET"])
            .output()?;
        if !filesystems.stdout.is_empty() {
            let members = Command::new("btrfs").args(["filesystem", "show", "--mounted", "--raw"])
                .output().context("Mounted btrfs filesystems require btrfs-progs on the host for member discovery")?;
            if !members.status.success() {
                bail!("Could not discover mounted btrfs members");
            }
            let members = String::from_utf8(members.stdout)?;
            let paths = btrfs_members(&members)?;
            for path in paths {
                let ancestors = Command::new("lsblk")
                    .args([
                        "--inverse",
                        "--noheadings",
                        "--raw",
                        "--output",
                        "MAJ:MIN",
                        "--",
                    ])
                    .arg(&path)
                    .output()?;
                if !ancestors.status.success() {
                    bail!("Could not inspect btrfs member {path}");
                }
                for id in String::from_utf8(ancestors.stdout)?.lines() {
                    devices.0.insert(id.trim().into(), path.clone());
                }
            }
        }
        fn has_root(devices: &[BlockDevice]) -> bool {
            devices.iter().any(|device| {
                device.mountpoints.iter().flatten().any(|p| p == "/") || has_root(&device.children)
            })
        }
        // btrfs findmnt identities are anonymous filesystem IDs rather than
        // block IDs; lsblk's mount attribution still identifies the root.
        if !devices.0.contains_key(&identity) && !has_root(&topology.blockdevices) {
            bail!(
                "Cannot identify the block devices backing the host root filesystem. Installing from overlay/container/network roots is unsupported; use the live ISO."
            );
        }
        Ok(devices)
    }

    fn from_topology(topology: &Topology) -> Self {
        fn visit(device: &BlockDevice, protected: &mut BTreeMap<String, String>) -> bool {
            // Visit every child, including repeated LVM/MD nodes under multiple
            // disks. All physical ancestors of an in-use mapping are protected.
            let mut used = device
                .mountpoints
                .iter()
                .flatten()
                .any(|value| !value.is_empty());
            for child in &device.children {
                used |= visit(child, protected);
            }
            if used {
                protected.insert(device.identity.clone(), device.name.clone());
            }
            used
        }
        let mut protected = BTreeMap::new();
        for device in &topology.blockdevices {
            visit(device, &mut protected);
        }
        Self(protected)
    }

    pub fn ensure_available(&self, path: &str) -> Result<()> {
        let metadata =
            std::fs::metadata(path).with_context(|| format!("Cannot inspect target {path}"))?;
        if !metadata.file_type().is_block_device() {
            bail!("Target is not a block device: {path}");
        }
        let device = metadata.rdev();
        let identity = format!("{}:{}", nix::libc::major(device), nix::libc::minor(device));
        if let Some(source) = self.0.get(&identity) {
            bail!(
                "Cannot install onto {path}: {source} was in use by the host when the bootstrap started. Unmount a spare disk on the host before starting the installer; use the live ISO for the host's own disk."
            );
        }
        // Mounts in the source namespace can change after capture(). Linux's
        // exclusive block-device claim sees mounts, swap and DM/MD holders
        // across all namespaces, including claims on a disk's partitions.
        // Do not hold this claim: it would also prevent our own partitioning,
        // formatting and mounts. This is an idle check, not a reservation;
        // callers must recheck at the write boundary and the target must not
        // be used concurrently by another process.
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_EXCL)
            .open(path)
            .with_context(|| format!(
                "Cannot claim target {path} exclusively. It may have been mounted or activated since the installer started, including in another mount namespace. Release it on the host before continuing."
            ))?;
        Ok(())
    }
}

fn btrfs_members(listing: &str) -> Result<Vec<String>> {
    let mut members = Vec::new();
    for line in listing.lines().map(str::trim) {
        if line.starts_with("devid ") {
            let (_, path) = line
                .split_once(" path ")
                .context("Cannot parse btrfs member discovery")?;
            if !path.starts_with("/dev/") {
                bail!("Unavailable btrfs member: {path}");
            }
            members.push(path.to_owned());
        }
        if line.contains("Some devices missing") {
            bail!("Cannot safely protect a degraded host btrfs filesystem");
        }
    }
    if members.is_empty() {
        bail!("No members found for mounted host btrfs filesystems");
    }
    Ok(members)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires root, loop devices, sfdisk, mkfs.ext4, and mount namespaces"]
    fn a_later_mount_in_another_namespace_refuses_the_whole_disk() -> Result<()> {
        use std::io::{BufRead, BufReader};
        use std::process::{Child, Stdio};

        anyhow::ensure!(
            nix::unistd::Uid::effective().is_root(),
            "Run this test as root"
        );
        let directory = tempfile::tempdir()?;
        let image = directory.path().join("disk.img");
        std::fs::File::create(&image)?.set_len(64 * 1024 * 1024)?;
        let layout = directory.path().join("layout");
        std::fs::write(&layout, "label: dos\n,32M,L\n")?;
        super::super::run(
            Command::new("sfdisk")
                .arg(&image)
                .stdin(std::fs::File::open(layout)?),
        )?;
        let output = Command::new("losetup")
            .args(["--find", "--show", "--partscan"])
            .arg(&image)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "Could not allocate a test loop device"
        );

        // Release only this test's loop device, including after an assertion
        // failure. Killing the namespace owner also releases its mount.
        struct LoopDevice {
            path: String,
            namespace: Option<Child>,
        }
        impl Drop for LoopDevice {
            fn drop(&mut self) {
                if let Some(child) = &mut self.namespace {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                let _ = Command::new("losetup")
                    .args(["--detach", &self.path])
                    .status();
            }
        }
        let mut device = LoopDevice {
            path: String::from_utf8(output.stdout)?.trim().into(),
            namespace: None,
        };
        let protected = ProtectedDevices::default();
        protected.ensure_available(&device.path)?;
        let partition = format!("{}p1", device.path);
        super::super::run(Command::new("mkfs.ext4").args(["-q", "-F", &partition]))?;
        let target = directory.path().join("mnt");
        std::fs::create_dir(&target)?;
        device.namespace = Some(
            Command::new("unshare")
                .args([
                    "--mount",
                    "--propagation",
                    "private",
                    "sh",
                    "-eu",
                    "-c",
                    "mount \"$1\" \"$2\"; echo ready; read -r done",
                    "sh",
                ])
                .arg(&partition)
                .arg(&target)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?,
        );
        let child = device
            .namespace
            .as_mut()
            .context("Missing namespace owner")?;
        let mut ready = String::new();
        BufReader::new(child.stdout.take().context("Missing readiness pipe")?)
            .read_line(&mut ready)?;
        anyhow::ensure!(
            ready.trim() == "ready",
            "Test namespace could not mount its partition"
        );
        // The original protection snapshot is empty and our own namespace
        // cannot see the mount. Only the live kernel claim can refuse this.
        let error = protected.ensure_available(&device.path).unwrap_err();
        assert!(error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.raw_os_error() == Some(nix::libc::EBUSY))
        }));
        child.kill()?;
        child.wait()?;
        protected.ensure_available(&device.path)?;
        Ok(())
    }

    #[test]
    fn btrfs_member_discovery_refuses_missing_members() {
        assert_eq!(btrfs_members("  devid    1 size 100 used 20 path /dev/sda1\n  devid 2 size 100 used 20 path /dev/sdb1\n").unwrap(), ["/dev/sda1", "/dev/sdb1"]);
        assert!(btrfs_members("devid 2 size 100 used 20 path missing").is_err());
        assert!(btrfs_members("*** Some devices missing").is_err());
    }

    #[test]
    fn protects_all_ancestors_of_mounts_and_swap_including_spanning_lvm() {
        let topology: Topology = serde_json::from_value(serde_json::json!({"blockdevices": [
            {"name":"/dev/sda", "maj:min":"8:0", "mountpoints":[null], "children":[
                {"name":"/dev/sda1", "maj:min":"8:1", "children":[
                    {"name":"/dev/mapper/root", "maj:min":"253:0", "mountpoints":["/"]}
                ]}
            ]},
            {"name":"/dev/sdb", "maj:min":"8:16", "children":[
                {"name":"/dev/mapper/root", "maj:min":"253:0", "mountpoints":["/"]}
            ]},
            {"name":"/dev/sdc", "maj:min":"8:32", "children":[
                {"name":"/dev/sdc1", "maj:min":"8:33", "mountpoints":["[SWAP]"]}
            ]},
            {"name":"/dev/sdd", "maj:min":"8:48", "mountpoints":[null]}
        ]}))
        .unwrap();
        let protected = ProtectedDevices::from_topology(&topology);
        for id in ["8:0", "8:1", "253:0", "8:16", "8:32", "8:33"] {
            assert!(protected.0.contains_key(id), "unprotected {id}");
        }
        assert!(!protected.0.contains_key("8:48"));
    }
}
