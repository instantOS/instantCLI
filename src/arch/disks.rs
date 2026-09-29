use anyhow::Result;
use serde_json::Value;
use std::process::Command;

use crate::arch::engine::{DataKey, DevicePath};
use crate::menu_utils::FzfPreview;
use crate::preview::{PreviewId, preview_command};

/// The running root device, if it resolves to a valid device path. Container
/// roots such as `overlay` and missing `findmnt` results yield `None`.
///
/// A [`DevicePath`], not a [`DiskPath`]: on a partitioned system the device
/// holding `/` is a partition, and a `DiskPath` would reject it and report no
/// root at all.
pub fn root_device() -> Option<DevicePath> {
    get_root_device()
        .ok()
        .flatten()
        .and_then(|device| DevicePath::parse(&device).ok())
}

/// The physical disk holding the running root filesystem. See [`root_device`].
pub fn boot_disk() -> Option<DevicePath> {
    get_boot_disk()
        .ok()
        .flatten()
        .and_then(|disk| DevicePath::parse(&disk).ok())
}

/// Strip btrfs subvolume annotations from `findmnt` sources (`/dev/sda1[/@]`).
fn bare_device(value: &str) -> &str {
    value.split('[').next().unwrap_or(value).trim()
}

/// Get the current root filesystem device (e.g., /dev/mapper/vg-root, /dev/sda2)
pub fn get_root_device() -> Result<Option<String>> {
    let output = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "/"])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let root_device = bare_device(&String::from_utf8_lossy(&output.stdout)).to_string();
    if root_device.is_empty() {
        return Ok(None);
    }

    Ok(Some(root_device))
}

/// Get the physical disk that contains the current root filesystem.
///
/// Falls back to the root device's partition name when `lsblk` cannot see its
/// parent disk, as can happen in a mount namespace. Returning `None` there
/// would leave the running-disk guard without a disk to refuse.
pub fn get_boot_disk() -> Result<Option<String>> {
    let root_device = match get_root_device()? {
        Some(device) => device,
        None => return Ok(None),
    };

    let lsblk_output = Command::new("lsblk").args(["-J"]).output()?;
    if !lsblk_output.status.success() {
        return Ok(None);
    }

    let lsblk_json: Value = serde_json::from_slice(&lsblk_output.stdout)?;

    fn find_physical_disk(blockdevices: &[Value], target_name: &str) -> Option<String> {
        for device in blockdevices {
            let name = device.get("name")?.as_str()?;
            let device_type = device.get("type")?.as_str()?;

            let device_path = if name.contains('/') {
                name.to_string()
            } else {
                format!("/dev/{}", name)
            };

            if device_path == target_name || name == target_name.trim_start_matches("/dev/") {
                if device_type == "disk" {
                    return Some(device_path);
                }

                return find_parent_disk(blockdevices, name);
            }

            if let Some(children) = device.get("children").and_then(|c| c.as_array())
                && let Some(disk) = find_physical_disk(children, target_name)
            {
                return Some(disk);
            }
        }
        None
    }

    fn find_parent_disk(blockdevices: &[Value], target_name: &str) -> Option<String> {
        for device in blockdevices {
            let device_type = device.get("type")?.as_str()?;

            if device_type == "disk" {
                if let Some(children) = device.get("children").and_then(|c| c.as_array())
                    && contains_device(children, target_name)
                {
                    let name = device.get("name")?.as_str()?;
                    let disk_path = if name.contains('/') {
                        name.to_string()
                    } else {
                        format!("/dev/{}", name)
                    };
                    return Some(disk_path);
                }
            } else if let Some(children) = device.get("children").and_then(|c| c.as_array())
                && let Some(disk) = find_parent_disk(children, target_name)
            {
                return Some(disk);
            }
        }
        None
    }

    fn contains_device(children: &[Value], target_name: &str) -> bool {
        for child in children {
            if let Some(name) = child.get("name").and_then(|n| n.as_str())
                && name == target_name
            {
                return true;
            }
            if let Some(grandchildren) = child.get("children").and_then(|c| c.as_array())
                && contains_device(grandchildren, target_name)
            {
                return true;
            }
        }
        false
    }

    if let Some(blockdevices) = lsblk_json.get("blockdevices").and_then(|b| b.as_array())
        && let Some(disk) = find_physical_disk(blockdevices, &root_device)
    {
        return Ok(Some(disk));
    }

    // `lsblk` can miss a parent disk in a mount namespace.
    Ok(disk_of_partition(&root_device))
}

/// Infer a parent disk from a partition suffix, such as `/dev/sda2` or
/// `/dev/nvme0n1p2`. Returns `None` when no suffix is recognized.
fn disk_of_partition(device: &str) -> Option<String> {
    let (parent, name) = device.rsplit_once('/')?;
    if let Some(index) = name.rfind('p')
        && index > 0
        && name[index + 1..].chars().all(|c| c.is_ascii_digit())
        && !name[index + 1..].is_empty()
    {
        return Some(format!("{parent}/{}", &name[..index]));
    }
    // A trailing digit run is a partition only for the non-NVMe naming
    // scheme: `sda2`, `vdb1`, `md0p` is handled above.
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 && !is_nvme_whole_disk(name) {
        return Some(format!("{parent}/{}", &name[..name.len() - digits]));
    }
    None
}

/// Whether this is an NVMe whole-disk name (`nvme0n1`). Its partitions use
/// `p`, so `nvme0n11` is a different disk, not a partition of `nvme0n1`.
fn is_nvme_whole_disk(name: &str) -> bool {
    let base = name
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(name);
    let Some(marker) = base.rfind('n') else {
        return false;
    };
    let tail = &base[marker + 1..];
    !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit())
}

/// Whether `source` is `disk` itself or one of its partitions.
///
/// Checks partition suffixes so `/dev/sdaa1` is not mistaken for a partition
/// of `/dev/sda`. Handles numbered and `p`-separated partition names.
pub fn is_partition_of(disk: &str, source: &str) -> bool {
    let disk = disk.trim_end_matches('/');
    let Some(suffix) = source.strip_prefix(disk) else {
        return false;
    };
    if suffix.is_empty() {
        return true;
    }
    if suffix.chars().all(|c| c.is_ascii_digit()) {
        // `/dev/sda` → `/dev/sda1` is a partition; `/dev/nvme0n1` →
        // `/dev/nvme0n11` is a different disk.
        return !is_nvme_whole_disk(disk);
    }
    // Only the `p`-separated form is left, and it needs something after the
    // `p` so `/dev/sda` never claims `/dev/sdapart`.
    match suffix.strip_prefix('p') {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()),
        None => false,
    }
}

pub fn get_mounted_partitions(disk: &str) -> Result<Vec<String>> {
    let output = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE"])
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut mounted = Vec::new();

    for line in stdout.lines() {
        let source = line.trim();
        if is_partition_of(disk, source) {
            mounted.push(source.to_string());
        }
    }

    Ok(mounted)
}

pub fn get_swap_partitions(disk: &str) -> Result<Vec<String>> {
    let swaps = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
    let mut swap_parts = Vec::new();

    for line in swaps.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(filename) = parts.first()
            && is_partition_of(disk, filename)
        {
            swap_parts.push(filename.to_string());
        }
    }

    Ok(swap_parts)
}

/// Refuse to take a disk the running system depends on.
///
/// Called before `umount` or `swapoff`, including for hand-authored plans that
/// bypass the wizard's disk question.
pub fn ensure_not_running_disk(disk: &str) -> Result<()> {
    // A target that is not a device path cannot be the running device, so
    // there is nothing to refuse. Parsed as a `DevicePath`, because the device
    // holding `/` is a partition on a partitioned system and a `DiskPath`
    // would reject it — skipping the guard on exactly those hosts.
    let Some(target) = DevicePath::parse(disk).ok() else {
        return Ok(());
    };
    if let Some(conflict) = crate::arch::host::running_disk_conflict(
        target.as_str(),
        root_device().as_ref(),
        boot_disk().as_ref(),
    ) {
        anyhow::bail!(
            "{}",
            crate::arch::host::running_disk_message(disk, conflict)
        );
    }
    Ok(())
}

/// Result of preparing a disk for installation
#[derive(Debug, Default)]
pub struct DiskPrepareResult {
    pub unmounted: Vec<String>,
    pub swapoff: Vec<String>,
}

/// Prepare a disk for installation by unmounting all partitions and disabling swap
pub fn prepare_disk(disk: &str) -> Result<DiskPrepareResult> {
    let mut result = DiskPrepareResult::default();

    // Refuse the running disk before touching anything. `umount`ing the
    // device the installer is executing from does not fail cleanly — it
    // either fails with EBUSY or, with a lazy unmount, leaves every open file
    // descriptor pointing into a filesystem the install is about to erase.
    ensure_not_running_disk(disk)?;

    for partition in get_mounted_partitions(disk)? {
        let status = Command::new("umount").arg(&partition).status()?;
        if status.success() {
            result.unmounted.push(partition);
        } else {
            anyhow::bail!("Failed to unmount {}", partition);
        }
    }

    for partition in get_swap_partitions(disk)? {
        let status = Command::new("swapoff").arg(&partition).status()?;
        if status.success() {
            result.swapoff.push(partition);
        } else {
            anyhow::bail!("Failed to swapoff {}", partition);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_root_device() {
        // This test will only work on a running Linux system
        // It should be able to find the root device
        if cfg!(target_os = "linux") {
            let result = get_root_device();
            assert!(result.is_ok());

            match result.unwrap() {
                Some(device) => {
                    // In containers, the root device might be "overlay" or similar, so we don't assert /dev/ prefix
                    assert!(!device.is_empty());
                    println!("Root device detected: {}", device);

                    // Most common formats start with /dev/, but some environments
                    // (containers, network mounts, etc.) may return different formats
                    // The important thing is that we get a valid device identifier
                }
                None => {
                    // This might happen in some container environments
                    println!("No root device detected (might be normal in containers)");
                }
            }
        }
    }

    #[test]
    fn test_get_boot_disk() {
        // This test will only work on a running Linux system
        if cfg!(target_os = "linux") {
            let result = get_boot_disk();
            assert!(result.is_ok());

            match result.unwrap() {
                Some(disk) => {
                    assert!(disk.starts_with("/dev/"));
                    assert!(!disk.contains("mapper")); // Should be a physical disk, not logical volume
                    println!("Boot disk detected: {}", disk);
                }
                None => {
                    // This might happen in some container environments
                    println!("No boot disk detected (might be normal in containers)");
                }
            }
        }
    }
}

#[cfg(test)]
mod partition_match_tests {
    use super::{disk_of_partition, is_partition_of};

    #[test]
    fn a_disk_matches_its_own_partitions() {
        assert!(is_partition_of("/dev/sda", "/dev/sda"));
        assert!(is_partition_of("/dev/sda", "/dev/sda1"));
        assert!(is_partition_of("/dev/sda", "/dev/sda12"));
        assert!(is_partition_of("/dev/nvme0n1", "/dev/nvme0n1p1"));
        assert!(is_partition_of("/dev/nvme0n1", "/dev/nvme0n1p15"));
        assert!(is_partition_of("/dev/md0", "/dev/md0p2"));
    }

    #[test]
    fn a_disk_does_not_match_a_longer_disk_name() {
        // A prefix match would let `/dev/sda` claim `/dev/sdaa1`, so a machine
        // with a second disk whose name merely extends the first would abort
        // with a spurious "disk in use".
        assert!(!is_partition_of("/dev/sda", "/dev/sdaa"));
        assert!(!is_partition_of("/dev/sda", "/dev/sdaa1"));
        assert!(!is_partition_of("/dev/nvme0n1", "/dev/nvme0n11"));
        assert!(!is_partition_of("/dev/nvme0n1", "/dev/nvme0n11p1"));
        assert!(!is_partition_of("/dev/sda", "/dev/sdb1"));
        assert!(!is_partition_of("/dev/sda", "/dev/sdaa1p2"));
    }

    #[test]
    fn an_unrelated_device_never_matches() {
        assert!(!is_partition_of("/dev/sda", "overlay"));
        assert!(!is_partition_of("/dev/sda", "/dev/mapper/vg-root"));
        assert!(!is_partition_of("/dev/sda", "tmpfs"));
        // A `p` with nothing after it is not a partition.
        assert!(!is_partition_of("/dev/sda", "/dev/sdap"));
    }

    #[test]
    fn a_partition_name_yields_its_whole_disk() {
        // The fallback for when `lsblk` cannot place the running root device,
        // which happens inside a container's mount namespace. Without it the
        // disk guard fails open on the one disk that must never be selected.
        assert_eq!(disk_of_partition("/dev/vda1").as_deref(), Some("/dev/vda"));
        assert_eq!(disk_of_partition("/dev/sda12").as_deref(), Some("/dev/sda"));
        assert_eq!(
            disk_of_partition("/dev/nvme0n1p2").as_deref(),
            Some("/dev/nvme0n1")
        );
        assert_eq!(disk_of_partition("/dev/md0p3").as_deref(), Some("/dev/md0"));
    }

    #[test]
    fn a_whole_disk_name_yields_nothing() {
        // Returning the device itself would make `get_boot_disk` report the
        // root partition as the disk, which is exactly the confusion the
        // guard must not have.
        assert_eq!(disk_of_partition("/dev/vda"), None);
        assert_eq!(disk_of_partition("/dev/nvme0n1"), None);
        assert_eq!(disk_of_partition("/dev/sda"), None);
        // Not a `/dev` path at all: a network root, tmpfs, an overlay.
        assert_eq!(disk_of_partition("overlay"), None);
        assert_eq!(disk_of_partition("/dev/mapper/vg-root"), None);
    }
}

#[cfg(test)]
mod prepare_disk_guard_tests {
    use super::{bare_device, ensure_not_running_disk, root_device};
    use crate::arch::engine::{DevicePath, DiskPath};
    use crate::arch::host::TargetRelation;

    fn disk(value: &str) -> DiskPath {
        DiskPath::parse(value).expect("test device path")
    }

    fn device(value: &str) -> DevicePath {
        DevicePath::parse(value).expect("test device path")
    }

    #[test]
    fn the_btrfs_subvolume_annotation_is_stripped_at_the_boundary() {
        // findmnt reports `/dev/nvme0n1p2[/@]` for a btrfs root. Left attached,
        // it defeats every comparison against the running device.
        assert_eq!(bare_device("/dev/nvme0n1p2[/@]"), "/dev/nvme0n1p2");
        assert_eq!(bare_device("/dev/sda2"), "/dev/sda2");
    }

    #[test]
    fn a_root_that_is_not_a_device_cannot_be_an_install_target() {
        // Inside a container the root source is `overlay`, not a block device.
        // It can never equal an install target, so the guard has nothing to
        // refuse — and must not refuse every disk instead.
        assert!(DevicePath::parse("overlay").is_err());
        assert!(DevicePath::parse("tmpfs").is_err());
    }

    #[test]
    fn a_partitioned_root_is_still_reported_as_a_device() {
        // The root device on a partitioned system *is* a partition. Holding it
        // as a disk path would fail the whole-disk check and report no root at
        // all, leaving the guard with only the boot disk to catch the conflict.
        let partitioned_root = root_device();
        if let Some(root) = &partitioned_root {
            assert!(
                !crate::common::blockdev::classify_device_path(root.as_str())
                    .eq(&crate::common::blockdev::DeviceKind::Disk),
                "expected a non-disk root on this host, got {root:?}"
            );
        }
    }

    #[test]
    fn the_guard_reports_the_root_device_before_the_boot_disk() {
        // A message that names the wrong device sends the user looking in the
        // wrong place.
        assert_eq!(
            crate::arch::host::running_disk_conflict(
                disk("/dev/nvme0n1").as_str(),
                Some(&device("/dev/nvme0n1")),
                Some(&device("/dev/nvme0n1"))
            ),
            Some(TargetRelation::RootDevice)
        );
    }

    #[test]
    fn a_spare_disk_is_never_refused() {
        // A spare disk is installable from a running system. `root_device`
        // reads the machine, so pick a device name that cannot be the running
        // one.
        let spare = "/dev/ins-not-a-real-disk";
        if let Some(running) = root_device()
            && running.as_str() == spare
        {
            return;
        }
        assert!(ensure_not_running_disk(spare).is_ok());
    }

    #[test]
    fn the_running_root_is_refused_with_an_actionable_message() {
        let running = match root_device() {
            Some(device) => device,
            // Nothing resolvable on this host (container); the pure guard is
            // covered by `running_disk_conflict_names_which_running_device_matched`.
            None => return,
        };
        let error = ensure_not_running_disk(running.as_str())
            .unwrap_err()
            .to_string();
        assert!(error.contains(running.as_str()));
        assert!(error.contains("live ISO"));
        assert!(
            error.contains("different disk"),
            "the message must say what is possible: {error}"
        );
    }
}

/// Represents a disk entry with path and size information
#[derive(Clone, Debug)]
pub struct DiskEntry {
    /// Device path (e.g., /dev/sda)
    pub path: String,
    /// Human-readable size (e.g., "500 GiB")
    pub size: String,
}

impl DiskEntry {
    pub fn new(path: String, size: String) -> Self {
        Self { path, size }
    }
}

impl crate::menu_utils::FzfSelectable for DiskEntry {
    fn fzf_display_text(&self) -> String {
        format!("{} ({})", self.path, self.size)
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Command(preview_command(PreviewId::Disk))
    }

    fn fzf_key(&self) -> String {
        self.path.clone()
    }
}

pub struct DisksKey;

impl DataKey for DisksKey {
    type Value = Vec<DiskEntry>;
    const KEY: &'static str = "disks";
}

pub struct DiskProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for DiskProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        use crate::arch::dualboot::detect_disks;

        match detect_disks() {
            Ok(disk_infos) => {
                let disks: Vec<DiskEntry> = disk_infos
                    .into_iter()
                    .map(|info| {
                        let size = info.size_human();
                        DiskEntry::new(info.device, size)
                    })
                    .collect();

                if disks.is_empty() {
                    eprintln!("No disks found. Are you running with sudo?");
                }

                context.set::<DisksKey>(disks);
            }
            Err(e) => {
                eprintln!("Failed to detect disks: {}", e);
                // We don't fail the whole process here, just list no disks
                // This might allow the user to retry or debug
                context.set::<DisksKey>(Vec::new());
            }
        }

        Ok(())
    }
}
