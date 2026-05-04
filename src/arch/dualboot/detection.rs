//! Partition and OS detection for dual boot setup
//!
//! This module provides functionality to detect existing operating systems,
//! partition layouts, and resize feasibility for dual boot configurations.

use crate::arch::dualboot::os_detection::detect_os_from_info;
use crate::arch::dualboot::parsing;
use crate::arch::dualboot::types::format_size;
use crate::arch::dualboot::types::{
    DiskAnalysis, DiskInfo, MIN_ESP_SIZE, PartitionTableType, ResizeInfo,
};
use anyhow::Result;

/// Detect all disks and their partitions
pub fn detect_disks() -> Result<Vec<DiskInfo>> {
    let lsblk = crate::common::blockdev::load_lsblk(&[])?;
    detect_disks_from_lsblk(lsblk)
}

/// Internal implementation of disk detection from lsblk output
pub fn detect_disks_from_lsblk(
    lsblk: crate::common::blockdev::LsblkOutput,
) -> Result<Vec<DiskInfo>> {
    let mut disks = Vec::new();

    for device in &lsblk.blockdevices {
        if !device.is_disk() {
            continue;
        }

        if device.name.starts_with("loop") {
            continue;
        }

        let size_bytes = device.size.unwrap_or(0);
        let partition_table = match device
            .pttype
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "gpt" => PartitionTableType::GPT,
            "dos" | "mbr" => PartitionTableType::MBR,
            _ => PartitionTableType::Unknown,
        };

        let mut partitions = Vec::new();

        for child in &device.children {
            let child_value = child.to_json_value();
            if let Some(partition) =
                parsing::parse_partition(&child_value, detect_os_from_info, get_efi_resize_info)
            {
                partitions.push(partition);
            }
        }

        let total_partition_size: u64 = partitions.iter().map(|p| p.size_bytes).sum();
        let unpartitioned_space_bytes = size_bytes.saturating_sub(total_partition_size);

        // Calculate largest contiguous free space using sfdisk
        let device_path = device.path();
        let max_contiguous_free_space_bytes =
            get_largest_free_region(&device_path, Some(size_bytes)).unwrap_or(0);

        disks.push(DiskInfo {
            device: device_path,
            size_bytes,
            partition_table,
            partitions,
            unpartitioned_space_bytes,
            max_contiguous_free_space_bytes,
        });
    }

    Ok(disks)
}

/// Get the largest contiguous free region in bytes for a device (Helper wrapper)
fn get_largest_free_region(device: &str, disk_size_bytes: Option<u64>) -> Option<u64> {
    parsing::get_free_regions(device, disk_size_bytes)
        .ok()?
        .into_iter()
        .map(|r| r.size_bytes)
        .max()
}

/// Check dual boot feasibility for all detected disks
pub fn analyze_all_disks() -> Result<Vec<DiskAnalysis>> {
    let disks = detect_disks()?;

    let results = disks
        .into_iter()
        .map(|disk| {
            let feasibility = disk.check_disk_dualboot_feasibility();
            DiskAnalysis { disk, feasibility }
        })
        .collect();

    Ok(results)
}

/// Get resize info for EFI System Partition
fn get_efi_resize_info(size_bytes: u64) -> ResizeInfo {
    if size_bytes < MIN_ESP_SIZE {
        ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some(format!(
                "ESP is small ({}) - recommend 260MB+ for dual boot",
                format_size(size_bytes)
            )),
            prerequisites: vec![],
        }
    } else {
        ResizeInfo {
            can_shrink: false, // Don't shrink ESP
            min_size_bytes: None,
            reason: Some("Reuse for dual boot (do not reformat)".to_string()),
            prerequisites: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
        assert_eq!(format_size(1099511627776), "1.0 TB");
    }

    #[test]
    fn test_partition_table_display() {
        assert_eq!(format!("{}", PartitionTableType::GPT), "GPT");
        assert_eq!(format!("{}", PartitionTableType::MBR), "MBR");
        assert_eq!(format!("{}", PartitionTableType::Unknown), "Unknown");
    }

    #[test]
    fn test_resize_info_min_size_human() {
        let info = ResizeInfo {
            can_shrink: true,
            min_size_bytes: Some(1073741824),
            reason: None,
            prerequisites: vec![],
        };
        assert_eq!(info.min_size_human(), Some("1.0 GB".to_string()));

        let info_none = ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: None,
            prerequisites: vec![],
        };
        assert_eq!(info_none.min_size_human(), None);
    }

    #[test]
    fn test_detect_disks_from_lsblk_e2e() {
        use crate::arch::dualboot::test_utils::{GPT_DUAL_BOOT_SCRIPT, MB, TestDisk};
        use crate::common::blockdev::{BlockDevice, LsblkOutput};

        let disk = TestDisk::new(2048);
        disk.partition(GPT_DUAL_BOOT_SCRIPT);

        let lsblk = LsblkOutput {
            blockdevices: vec![BlockDevice {
                name: disk.path_str().to_string(),
                device_type: "disk".to_string(),
                size: Some(disk.size_mb * MB),
                fstype: None,
                uuid: None,
                label: None,
                mountpoint: None,
                pttype: Some("gpt".to_string()),
                parttype: None,
                children: vec![],
            }],
        };

        let disks = detect_disks_from_lsblk(lsblk).unwrap();
        assert_eq!(disks.len(), 1);
        let d = &disks[0];
        assert_eq!(d.device, disk.path_str());
        assert_eq!(d.partition_table, PartitionTableType::GPT);
        // Free space should be detected correctly by sfdisk
        assert!(d.max_contiguous_free_space_bytes > 300 * MB);
    }

    #[test]
    fn test_detect_disks_with_partitions_e2e() {
        use crate::arch::dualboot::test_utils::{GPT_DUAL_BOOT_SCRIPT, MB, TestDisk};
        use crate::common::blockdev::{BlockDevice, LsblkOutput};

        let disk = TestDisk::new(2048);
        disk.partition(GPT_DUAL_BOOT_SCRIPT);

        let lsblk = LsblkOutput {
            blockdevices: vec![BlockDevice {
                name: disk.path_str().to_string(),
                device_type: "disk".to_string(),
                size: Some(disk.size_mb * MB),
                fstype: None,
                uuid: None,
                label: None,
                mountpoint: None,
                pttype: Some("gpt".to_string()),
                parttype: None,
                children: vec![BlockDevice {
                    name: disk.path_str().to_string(), // Use disk path for partition in mock
                    device_type: "part".to_string(),
                    size: Some(100 * MB),
                    fstype: Some("vfat".to_string()),
                    uuid: None,
                    label: None,
                    mountpoint: None,
                    pttype: None,
                    parttype: Some("c12a7328-f81f-11d2-ba4b-00a0c93ec93b".to_string()),
                    children: vec![],
                }],
            }],
        };

        let disks = detect_disks_from_lsblk(lsblk).unwrap();
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].partitions.len(), 1);
        assert!(disks[0].partitions[0].is_efi);
    }
}
