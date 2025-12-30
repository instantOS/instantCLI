//! Partition and OS detection for dual boot setup
//!
//! This module provides functionality to detect existing operating systems,
//! partition layouts, and resize feasibility for dual boot configurations.

use crate::arch::dualboot::types::*;
use crate::arch::dualboot::resize;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

// Re-export types from types.rs for backward compatibility
pub use crate::arch::dualboot::types::{
    DiskInfo, PartitionInfo, FilesystemInfo, DetectedOS, OSType,
    ResizeInfo, FreeRegion, DualBootFeasibility, PartitionTableType,
    format_size, MIN_ESP_SIZE
};

// Re-export get_resize_info from resize module for backward compatibility
pub use resize::get_resize_info;

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
                if let Some(partition) = parse_partition(child) {
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
    get_free_regions(device, disk_size_bytes)
        .ok()?
        .into_iter()
        .map(|r| r.size_bytes)
        .max()
}

/// Represents a contiguous free space region on the disk (now defined in types.rs)

/// Get all contiguous free regions for a device
pub fn get_free_regions(device: &str, disk_size_bytes: Option<u64>) -> Result<Vec<FreeRegion>> {
    // Run sfdisk -J <device> to get partition table in JSON
    let output = Command::new("sfdisk")
        .args(["-J", device])
        .output()
        .context("Failed to run sfdisk -J")?;

    if !output.status.success() {
        // If it returns error (e.g. no partition table, empty disk), we assume no dual boot targets.
        // Dual boot requires an existing OS/partition structure to coexist with.
        return Ok(Vec::new());
    }

    let json_output = String::from_utf8_lossy(&output.stdout);
    calculate_free_regions_from_json(&json_output, disk_size_bytes)
}

#[derive(Debug, Deserialize)]
struct SfdiskOutput {
    partitiontable: SfdiskPartitionTable,
}

#[derive(Debug, Deserialize)]
struct SfdiskPartitionTable {
    // label: String, // e.g. "gpt", "dos" - unused for now
    firstlba: Option<u64>,
    lastlba: Option<u64>,
    size: Option<u64>, // total sectors (may be present for MBR)
    sectorsize: u64,
    partitions: Option<Vec<SfdiskPartition>>,
}

#[derive(Debug, Deserialize)]
struct SfdiskPartition {
    // node: String, // e.g. "test.img1" - unused
    start: u64,
    size: u64,
    // type: String, // unused
}

/// Calculate free regions by finding gaps in the partition table
fn calculate_free_regions_from_json(
    json: &str,
    disk_size_bytes: Option<u64>,
) -> Result<Vec<FreeRegion>> {
    let output: SfdiskOutput =
        serde_json::from_str(json).context("Failed to parse sfdisk JSON output")?;

    let pt = output.partitiontable;
    let sector_size = pt.sectorsize;
    let disk_size_sectors = disk_size_bytes.and_then(|b| {
        if sector_size > 0 {
            Some(b / sector_size)
        } else {
            None
        }
    });
    let mut partitions = pt.partitions.unwrap_or_default();

    let first_lba = pt
        .firstlba
        .or_else(|| partitions.iter().map(|p| p.start).min())
        .unwrap_or(0);

    let last_lba = pt
        .lastlba
        .or_else(|| pt.size.map(|s| s.saturating_sub(1)))
        .or_else(|| disk_size_sectors.map(|s| s.saturating_sub(1)))
        .or_else(|| {
            partitions
                .iter()
                .map(|p| p.start.saturating_add(p.size).saturating_sub(1))
                .max()
        })
        .map(|l| l.max(first_lba))
        .unwrap_or(first_lba);

    // Sort partitions by start sector to reliably find gaps
    partitions.sort_by_key(|p| p.start);

    let mut regions = Vec::new();
    let mut current_sector = first_lba;

    // Check gaps between partitions
    for partition in partitions {
        if partition.start > current_sector {
            let gap_sectors = partition.start - current_sector;
            // Only consider gaps large enough to be usable (e.g., > 1MB)
            // 1MB = 2048 sectors (at 512 bytes/sector)
            if gap_sectors > 2048 {
                regions.push(FreeRegion {
                    start: current_sector,
                    sectors: gap_sectors,
                    size_bytes: gap_sectors * sector_size,
                });
            }
        }
        current_sector = std::cmp::max(current_sector, partition.start + partition.size);
    }

    // Check gap at the end (between last partition and lastlba)
    if current_sector <= last_lba {
        let gap_sectors = (last_lba - current_sector) + 1; // lastlba is inclusive

        // Only consider gaps large enough (> 1MB)
        if gap_sectors > 2048 {
            regions.push(FreeRegion {
                start: current_sector,
                sectors: gap_sectors,
                size_bytes: gap_sectors * sector_size,
            });
        }
    }

    Ok(regions)
}

#[cfg(test)]
mod parsing_tests {
    use super::*;

    #[test]
    fn test_calculate_free_regions_simple_gap() {
        // 100 sectors total. Partition at 20-30.
        // Free: 0-19 (if firstlba=0), 31-100.
        // firstlba typically 34 or 2048 for GPT. Let's say 34.

        let json = r#"{
   "partitiontable": {
      "label": "gpt",
      "id": "A",
      "device": "test",
      "unit": "sectors",
      "firstlba": 34,
      "lastlba": 10000,
      "sectorsize": 512,
      "partitions": [
         {
            "node": "p1",
            "start": 5000,
            "size": 1000
         }
      ]
   }
}"#;
        // Gaps:
        // 1. 34 to 4999 (size 4966)
        // 2. 6000 to 10000 (size 4001)

        // Note: Our logic filters < 2048 sectors (1MB).
        // 4966 > 2048 -> Keep
        // 4001 > 2048 -> Keep

        let regions = calculate_free_regions_from_json(json, None).unwrap();
        assert_eq!(regions.len(), 2);

        assert_eq!(regions[0].start, 34);
        assert_eq!(regions[0].sectors, 4966);

        assert_eq!(regions[1].start, 6000);
        assert_eq!(regions[1].end, 10000);
    }

    #[test]
    fn test_calculate_free_regions_contiguous_user_scenario() {
        // User scenario:
        // 4GB free at start? (Assume existing partitions start late)
        // 5GB Linux
        // 2GB EFI
        // 26GB free at end

        // Let's model roughly in sectors (512b)
        // 1GB = 2,097,152 sectors
        // 4GB ~= 8,388,608 sectors

        // Case:
        // P1 Start: 8,388,608 + 2048 (offset). Size: 10,000,000
        // P2 Start: P1_End + 1. Size: 4,000,000
        // Disk End: Very large

        let json = r#"{
   "partitiontable": {
      "label": "gpt",
      "firstlba": 2048,
      "lastlba": 100000000,
      "sectorsize": 512,
      "partitions": [
         {
            "start": 10000000,
            "size": 10000000
         },
         {
            "start": 20000000,
            "size": 4000000
         }
      ]
   }
}"#;
        // Gaps:
        // 1. 2048 to 9,999,999. Size ~ 10M sectors (~5GB)
        // 2. 24,000,000 to 100,000,000. Size ~ 76M sectors (~38GB)

        let regions = calculate_free_regions_from_json(json, None).unwrap();

        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].start, 2048);
        assert!(regions[0].size_bytes > 4 * 1024 * 1024 * 1024); // > 4GB

        assert_eq!(regions[1].start, 24000000);
        assert!(regions[1].size_bytes > 20 * 1024 * 1024 * 1024); // > 20GB
    }

    #[test]
    fn test_calculate_free_regions_no_partitions() {
        // Empty partition table (e.g. freshly initialized GPT but no partitions)
        let json = r#"{
   "partitiontable": {
      "label": "gpt",
      "firstlba": 2048,
      "lastlba": 100000,
      "sectorsize": 512,
      "partitions": []
   }
}"#;
        let regions = calculate_free_regions_from_json(json, None).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start, 2048);
        assert_eq!(regions[0].end, 100000);
    }

    #[test]
    fn test_calculate_free_regions_missing_lba_fields_dos() {
        // DOS/MBR outputs do not include firstlba/lastlba; ensure we still parse without crashing.
        let json = r#"{
    "partitiontable": {
        "label": "dos",
        "device": "test",
        "unit": "sectors",
        "sectorsize": 512,
        "partitions": [
            {
                "start": 2048,
                "size": 4096
            }
        ]
    }
}"#;

        let regions = calculate_free_regions_from_json(json, None).unwrap();
        // With no reported disk end we cannot infer trailing free space; we just ensure parsing works.
        assert!(regions.is_empty());
    }

    #[test]
    fn test_calculate_free_regions_missing_lba_with_size() {
        // DOS/MBR with size field should infer disk end from size.
        let json = r#"{
    "partitiontable": {
        "label": "dos",
        "device": "test",
        "unit": "sectors",
        "size": 100000,
        "sectorsize": 512,
        "partitions": [
            {
                "start": 2048,
                "size": 4096
            }
        ]
    }
}"#;

        let regions = calculate_free_regions_from_json(json, None).unwrap();
        // Gap after the single partition to the end of disk.
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start, 2048 + 4096); // partition end + 1
        assert_eq!(regions[0].end, 99999);
        assert_eq!(regions[0].sectors, 99999 - (2048 + 4096) + 1);
    }
}

/// Check if a partition is feasible for dual boot installation
pub fn is_dualboot_feasible(partition: &PartitionInfo) -> bool {
    // Cannot resize EFI partitions
    if partition.is_efi {
        return false;
    }

    // Must be shrinkable
    let can_shrink = partition
        .resize_info
        .as_ref()
        .map(|r| r.can_shrink)
        .unwrap_or(false);

    if !can_shrink {
        return false;
    }

    // Must have enough space
    let min_existing = partition
        .resize_info
        .as_ref()
        .and_then(|r| r.min_size_bytes)
        .unwrap_or(0);

    partition.size_bytes.saturating_sub(min_existing) >= crate::arch::dualboot::MIN_LINUX_SIZE
}

/// Overall dual boot feasibility result for a disk (now defined in types.rs)

/// Check if dual boot is feasible on a specific disk
pub fn check_disk_dualboot_feasibility(disk: &DiskInfo) -> DualBootFeasibility {
    let feasible_partitions: Vec<String> = disk
        .partitions
        .iter()
        .filter(|p| is_dualboot_feasible(p))
        .map(|p| p.device.clone())
        .collect();

    // Check if we have enough unpartitioned space
    // We check CONTIGUOUS space to ensure we can actually create the partition
    let free_space_bytes = disk.max_contiguous_free_space_bytes;
    let has_unpartitioned_space = free_space_bytes >= crate::arch::dualboot::MIN_LINUX_SIZE;

    if feasible_partitions.is_empty() {
        if has_unpartitioned_space {
            return DualBootFeasibility {
                feasible: true,
                feasible_partitions: vec![], // No specific partition to resize, but disk is feasible
                reason: Some(format!(
                    "Unpartitioned space available: {}",
                    format_size(free_space_bytes)
                )),
            };
        }

        // Check if there are any partitions at all
        if disk.partitions.is_empty() {
            // If empty partitions AND not enough space (checked above), then disk is too small
            DualBootFeasibility {
                feasible: false,
                feasible_partitions: vec![],
                reason: Some(format!(
                    "Disk too small or full (Largest free region: {})",
                    format_size(disk.max_contiguous_free_space_bytes)
                )),
            }
        } else {
            // Check if there are shrinkable partitions but not enough space
            let shrinkable: Vec<_> = disk
                .partitions
                .iter()
                .filter(|p| {
                    !p.is_efi
                        && p.resize_info
                            .as_ref()
                            .map(|r| r.can_shrink)
                            .unwrap_or(false)
                })
                .collect();

            // Filter out partitions that are way too small (e.g. < 2GB) to be relevant candidates
            // This prevents misleading messages like "shrinkable partitions found" when only a tiny /boot exists
            let valid_candidates: Vec<_> = shrinkable
                .iter()
                .filter(|p| p.size_bytes >= 2 * 1024 * 1024 * 1024)
                .collect();

            if valid_candidates.is_empty() {
                DualBootFeasibility {
                    feasible: false,
                    feasible_partitions: vec![],
                    reason: Some(
                        "No suitable partitions found (too small or not shrinkable)".to_string(),
                    ),
                }
            } else {
                DualBootFeasibility {
                    feasible: false,
                    feasible_partitions: vec![],
                    reason: Some("Shrinkable partitions found, but none have enough free space for Linux (need 10GB)".to_string()),
                }
            }
        }
    } else {
        DualBootFeasibility {
            feasible: true,
            feasible_partitions,
            reason: None,
        }
    }
}

/// Check dual boot feasibility for all detected disks
pub fn check_all_disks_feasibility() -> Result<(Vec<DiskInfo>, Vec<(String, DualBootFeasibility)>)>
{
    let disks = detect_disks()?;
    let mut results = Vec::new();

    for disk in &disks {
        let feasibility = check_disk_dualboot_feasibility(disk);
        results.push((disk.device.clone(), feasibility));
    }

    Ok((disks, results))
}

/// Parse a partition from lsblk JSON
fn parse_partition(value: &Value) -> Option<PartitionInfo> {
    let name = value.get("name")?.as_str()?;
    let size_bytes = value.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

    let fs_type = value
        .get("fstype")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let filesystem = fs_type.map(|fs| FilesystemInfo {
        fs_type: fs.to_string(),
        uuid: value
            .get("uuid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        label: value
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    });

    let mount_point = value
        .get("mountpoint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Check if this is an EFI System Partition
    // MBR: 0xef, GPT: C12A7328-F81F-11D2-BA4B-00A0C93EC93B (case insensitive)
    let parttype = value.get("parttype").and_then(|v| v.as_str()).unwrap_or("");
    let is_efi = is_efi_partition(parttype);

    // Detect OS based on filesystem type and mount point
    let detected_os = if is_efi {
        Some(DetectedOS {
            os_type: OSType::Unknown,
            name: "EFI System Partition".to_string(),
        })
    } else {
        detect_os_from_info(&filesystem, &mount_point)
    };

    // Get resize info based on filesystem type and EFI status
    let device_path = format!("/dev/{}", name);
    let resize_info = if is_efi {
        Some(get_efi_resize_info(size_bytes))
    } else {
        filesystem
            .as_ref()
            .map(|fs| get_resize_info(&device_path, &fs.fs_type, mount_point.as_deref()))
    };

    Some(PartitionInfo {
        device: device_path,
        size_bytes,
        filesystem,
        detected_os,
        resize_info,
        mount_point,
        is_efi,
        partition_type: if parttype.is_empty() {
            None
        } else {
            Some(parttype.to_string())
        },
    })
}

/// Check if partition type indicates EFI System Partition
fn is_efi_partition(parttype: &str) -> bool {
    let pt = parttype.to_lowercase();
    // MBR type 0xef or ef
    if pt == "0xef" || pt == "ef" {
        return true;
    }
    // GPT GUID for EFI System Partition (case insensitive comparison)
    if pt == "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" {
        return true;
    }
    false
}

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

/// Detect OS based on filesystem info (heuristic, no mounting required)
fn detect_os_from_info(
    filesystem: &Option<FilesystemInfo>,
    mount_point: &Option<String>,
) -> Option<DetectedOS> {
    let fs = filesystem.as_ref()?;

    match fs.fs_type.as_str() {
        "ntfs" => {
            // NTFS is almost always Windows
            // Check label for hints
            let name = if let Some(label) = &fs.label {
                if label.to_lowercase().contains("windows") {
                    "Windows".to_string()
                } else {
                    "Windows (NTFS)".to_string()
                }
            } else {
                "Windows (NTFS)".to_string()
            };

            Some(DetectedOS {
                os_type: OSType::Windows,
                name,
            })
        }
        "ext4" | "ext3" | "ext2" | "btrfs" | "xfs" => {
            // Linux filesystems
            // Check if it's a root partition
            if mount_point.as_ref().is_some_and(|mp| mp == "/") {
                // Try to read /etc/os-release for the current system
                if let Ok(os_release) = std::fs::read_to_string("/etc/os-release")
                    && let Some(name) = parse_os_release_field(&os_release, "PRETTY_NAME")
                {
                    return Some(DetectedOS {
                        os_type: OSType::Linux,
                        name,
                    });
                }
            }

            Some(DetectedOS {
                os_type: OSType::Linux,
                name: format!("Linux ({})", fs.fs_type),
            })
        }
        "apfs" | "hfsplus" | "hfs" => Some(DetectedOS {
            os_type: OSType::MacOS,
            name: "macOS".to_string(),
        }),
        _ => None,
    }
}

/// Parse a field from /etc/os-release format
fn parse_os_release_field(content: &str, field: &str) -> Option<String> {
    for line in content.lines() {
        if line.starts_with(field)
            && let Some(value) = line.strip_prefix(&format!("{}=", field))
        {
            // Remove quotes if present
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

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
