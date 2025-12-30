//! Partition and OS detection for dual boot setup
//!
//! This module provides functionality to detect existing operating systems,
//! partition layouts, and resize feasibility for dual boot configurations.

use crate::arch::dualboot::feasibility;
use crate::arch::dualboot::os_detection::detect_os_from_info;
use crate::arch::dualboot::parsing;
use crate::arch::dualboot::types::{DiskInfo, DualBootFeasibility, MIN_ESP_SIZE, PartitionTableType, ResizeInfo};
use crate::arch::dualboot::types::format_size;
use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

/// Detect all disks and their partitions
pub fn detect_disks() -> Result<Vec<DiskInfo>> {
    let output = Command::new("lsblk")
        .args([
            "-J",
            "-b",
            "-o",
            "NAME,SIZE,TYPE,FSTYPE,UUID,LABEL,MOUNTPOINT,PTTYPE,PARTTYPE",
        ])
        .output()
        .context("Failed to run lsblk")?;

    if !output.status.success() {
        anyhow::bail!("lsblk failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    let json: Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse lsblk JSON output")?;

    let blockdevices = json
        .get("blockdevices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("No blockdevices in lsblk output"))?;

    let mut disks = Vec::new();

    for device in blockdevices {
        let device_type = device.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Only process disk devices, skip loop, rom, etc.
        if device_type != "disk" {
            continue;
        }

        let name = device.get("name").and_then(|v| v.as_str()).unwrap_or("");

        // Skip loop devices
        if name.starts_with("loop") {
            continue;
        }

        let size_bytes = device.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        let pttype = device.get("pttype").and_then(|v| v.as_str()).unwrap_or("");

        let partition_table = match pttype.to_lowercase().as_str() {
            "gpt" => PartitionTableType::GPT,
            "dos" | "mbr" => PartitionTableType::MBR,
            _ => PartitionTableType::Unknown,
        };

        let mut partitions = Vec::new();

        if let Some(children) = device.get("children").and_then(|v| v.as_array()) {
            for child in children {
                // Use the new parse_partition signature with closures for OS detection and EFI resize
                if let Some(partition) =
                    parsing::parse_partition(child, detect_os_from_info, get_efi_resize_info)
                {
                    partitions.push(partition);
                }
            }
        }

        let total_partition_size: u64 = partitions.iter().map(|p| p.size_bytes).sum();
        let unpartitioned_space_bytes = size_bytes.saturating_sub(total_partition_size);

        // Calculate largest contiguous free space using sfdisk
        let device_path = format!("/dev/{}", name);
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

// Parsing functions have been moved to parsing/ module
// Feasibility checking functions have been moved to feasibility/ module
// OS detection functions have been moved to os_detection/ module

/// Check dual boot feasibility for all detected disks
pub fn check_all_disks_feasibility() -> Result<(Vec<DiskInfo>, Vec<(String, DualBootFeasibility)>)>
{
    let disks = detect_disks()?;
    let mut results = Vec::new();

    for disk in &disks {
        let feasibility = feasibility::check_disk_dualboot_feasibility(disk);
        results.push((disk.device.clone(), feasibility));
    }

    Ok((disks, results))
}

// parse_partition and is_efi_partition have been moved to parsing/lsblk.rs

/// Get resize info for EFI System Partition
fn get_efi_resize_info(size_bytes: u64) -> ResizeInfo {
    // Use module-level MIN_ESP_SIZE constant

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

// detect_os_from_info and parse_os_release_field have been moved to os_detection/ module

// Resize functions have been moved to resize/ module
// get_resize_info, get_ntfs_resize_info, parse_ntfs_min_size,
// get_ext_resize_info, parse_dumpe2fs_field,
// get_btrfs_resize_info, parse_btrfs_min_free, parse_btrfs_device_size, parse_btrfs_used
// are now in resize/ntfs.rs, resize/ext.rs, resize/btrfs.rs, resize/other.rs

#[cfg(test)]
mod tests {
    use std::io::Write;

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
    fn test_parse_os_release_field() {
        let content = r#"NAME="Arch Linux"
PRETTY_NAME="Arch Linux"
ID=arch
BUILD_ID=rolling
VERSION_ID="TEMPLATE_VERSION_ID"
ANSI_COLOR="38;2;23;147;209"
HOME_URL="https://archlinux.org/"
"#;
        assert_eq!(
            parse_os_release_field(content, "PRETTY_NAME"),
            Some("Arch Linux".to_string())
        );
        assert_eq!(
            parse_os_release_field(content, "ID"),
            Some("arch".to_string())
        );
        assert_eq!(parse_os_release_field(content, "NONEXISTENT"), None);
    }

    // NTFS resize tests have been moved to resize/ntfs.rs

    #[test]
    fn test_partition_table_display() {
        assert_eq!(format!("{}", PartitionTableType::GPT), "GPT");
        assert_eq!(format!("{}", PartitionTableType::MBR), "MBR");
        assert_eq!(format!("{}", PartitionTableType::Unknown), "Unknown");
    }

    #[test]
    fn test_os_type_display() {
        assert_eq!(format!("{}", OSType::Windows), "Windows");
        assert_eq!(format!("{}", OSType::Linux), "Linux");
        assert_eq!(format!("{}", OSType::MacOS), "macOS");
        assert_eq!(format!("{}", OSType::Unknown), "Unknown");
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

    // Btrfs tests have been moved to resize/btrfs.rs

    const MB: u64 = 1024 * 1024;

    fn create_image_with_sfdisk(size_mb: u64, script: &str) -> tempfile::TempPath {
        let file = tempfile::NamedTempFile::new().expect("temp image");
        file.as_file()
            .set_len(size_mb * MB)
            .expect("set image size");

        let path = file.into_temp_path();
        let mut child = Command::new("sfdisk")
            .arg("--no-reread")
            .arg("--quiet")
            .arg(path.to_str().expect("path"))
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("spawn sfdisk");

        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("write script");

        let status = child.wait().expect("wait sfdisk");
        assert!(status.success(), "sfdisk failed: {status}");
        path
    }

    #[test]
    fn sfdisk_detects_end_gap_on_gpt_image() {
        // 100MB image with two 20MB partitions leaves a large gap at the end.
        let script = "label: gpt\n,20M\n,20M\n";
        let img = create_image_with_sfdisk(100, script);

        let regions = get_free_regions(img.to_str().expect("path"), None).expect("regions");
        assert!(!regions.is_empty());

        let max_gap = regions.iter().map(|r| r.size_bytes).max().unwrap();
        // Expect roughly 60MB free (allow wide tolerance for alignment/padding).
        assert!(
            max_gap > 50 * MB && max_gap < 75 * MB,
            "gap was {} bytes",
            max_gap
        );
    }

    #[test]
    fn sfdisk_detects_middle_gap_between_partitions() {
        // 200MB image: 32MB partition, ~60MB gap, 32MB partition, remaining free at end.
        let script = "label: gpt\n,32M\nstart=120M,size=32M\n";
        let img = create_image_with_sfdisk(200, script);

        let regions = get_free_regions(img.to_str().expect("path"), None).expect("regions");
        assert!(regions.len() >= 2);

        let mut sizes: Vec<u64> = regions.iter().map(|r| r.size_bytes).collect();
        sizes.sort_unstable();

        let largest = *sizes.last().unwrap();
        let second = sizes[sizes.len() - 2];

        // Middle gap should be comfortably above the 1MB cutoff; allow wide tolerance for alignment.
        assert!(
            second > 30 * MB && second < 120 * MB,
            "middle gap {} bytes",
            second
        );
        assert!(
            largest > 40 * MB && largest < 140 * MB,
            "end gap {} bytes",
            largest
        );
    }

    #[test]
    fn sfdisk_detects_small_gaps_filtered_out() {
        // 50MB image with tiny 512KB gap between partitions should be ignored (<1MB threshold).
        let script = "label: gpt\n,10M\nstart=12M,size=10M\n";
        let img = create_image_with_sfdisk(50, script);

        let regions = get_free_regions(img.to_str().expect("path"), None).expect("regions");

        // Only the main trailing gap should remain; tiny gap is below threshold.
        assert_eq!(regions.len(), 1);
        let gap = regions[0].size_bytes;
        assert!(gap > 25 * MB && gap < 40 * MB, "trailing gap {} bytes", gap);
    }
}
