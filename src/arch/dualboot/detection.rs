//! Partition and OS detection for dual boot setup
//!
//! This module provides functionality to detect existing operating systems,
//! partition layouts, and resize feasibility for dual boot configurations.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;

/// Format bytes as human-readable size
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Information about a physical disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Device path (e.g., /dev/nvme0n1)
    pub device: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// Partition table type
    pub partition_table: PartitionTableType,
    /// List of partitions on this disk
    pub partitions: Vec<PartitionInfo>,
    /// Unpartitioned space in bytes (calculated)
    pub unpartitioned_space_bytes: u64,
    /// Largest contiguous unpartitioned space in bytes (detected via sfdisk)
    pub max_contiguous_free_space_bytes: u64,
}

/// Minimum ESP size for dual boot (260 MB recommended for multi-OS)
pub const MIN_ESP_SIZE: u64 = 260 * 1024 * 1024;

impl DiskInfo {
    /// Get human-readable size
    pub fn size_human(&self) -> String {
        format_size(self.size_bytes)
    }

    /// Check if disk already has enough unpartitioned space for Linux installation
    pub fn has_sufficient_free_space(&self) -> bool {
        self.max_contiguous_free_space_bytes >= crate::arch::dualboot::MIN_LINUX_SIZE
    }

    /// Find a suitable EFI partition for reuse in dual boot
    /// Returns the first ESP that is at least MIN_ESP_SIZE
    pub fn find_reusable_esp(&self) -> Option<&PartitionInfo> {
        self.partitions
            .iter()
            .find(|p| p.is_efi && p.size_bytes >= MIN_ESP_SIZE)
    }
}

/// Partition table type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PartitionTableType {
    GPT,
    MBR,
    Unknown,
}

impl std::fmt::Display for PartitionTableType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitionTableType::GPT => write!(f, "GPT"),
            PartitionTableType::MBR => write!(f, "MBR"),
            PartitionTableType::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Information about a partition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionInfo {
    /// Device path (e.g., /dev/nvme0n1p2)
    pub device: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// Filesystem information
    pub filesystem: Option<FilesystemInfo>,
    /// Detected operating system
    pub detected_os: Option<DetectedOS>,
    /// Resize feasibility information
    pub resize_info: Option<ResizeInfo>,
    /// Current mount point, if any
    pub mount_point: Option<String>,
    /// Whether this is an EFI System Partition
    pub is_efi: bool,
    /// Partition type code (e.g. 0x83, 0xef)
    pub partition_type: Option<String>,
}

impl PartitionInfo {
    /// Get human-readable size
    pub fn size_human(&self) -> String {
        format_size(self.size_bytes)
    }
}

/// Filesystem information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemInfo {
    /// Filesystem type (e.g., ntfs, ext4, vfat)
    pub fs_type: String,
    /// UUID
    pub uuid: Option<String>,
    /// Label
    pub label: Option<String>,
}

/// Detected operating system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedOS {
    /// Type of OS
    pub os_type: OSType,
    /// Human-readable name
    pub name: String,
}

/// Operating system type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OSType {
    Windows,
    Linux,
    MacOS,
    Unknown,
}

impl std::fmt::Display for OSType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OSType::Windows => write!(f, "Windows"),
            OSType::Linux => write!(f, "Linux"),
            OSType::MacOS => write!(f, "macOS"),
            OSType::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Resize feasibility information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeInfo {
    /// Whether the partition can be shrunk
    pub can_shrink: bool,
    /// Minimum size in bytes (if shrinkable)
    pub min_size_bytes: Option<u64>,
    /// Reason why it can or can't be resized
    pub reason: Option<String>,
    /// Prerequisites that must be met before resizing
    pub prerequisites: Vec<String>,
}

impl ResizeInfo {
    /// Get human-readable minimum size
    pub fn min_size_human(&self) -> Option<String> {
        self.min_size_bytes.map(format_size)
    }
}

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

/// Represents a contiguous free space region on the disk
#[derive(Debug, Clone, Copy)]
pub struct FreeRegion {
    /// Start sector
    pub start: u64,
    /// Number of sectors
    pub sectors: u64,
    /// Size in bytes
    pub size_bytes: u64,
}

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

/// Overall dual boot feasibility result for a disk
#[derive(Debug, Clone)]
pub struct DualBootFeasibility {
    /// Whether dual boot is feasible on this disk
    pub feasible: bool,
    /// List of partitions that could be used for dual boot
    pub feasible_partitions: Vec<String>,
    /// Reason why dual boot is not feasible (if applicable)
    pub reason: Option<String>,
}

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

/// Get resize information for a partition based on filesystem type
fn get_resize_info(device: &str, fs_type: &str, mount_point: Option<&str>) -> ResizeInfo {
    match fs_type {
        "ntfs" => get_ntfs_resize_info(device),
        "ext4" | "ext3" | "ext2" => get_ext_resize_info(device, mount_point),
        "btrfs" => get_btrfs_resize_info(mount_point),
        "xfs" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("XFS can only grow, not shrink".to_string()),
            prerequisites: vec![],
        },
        "vfat" | "fat32" | "fat16" | "exfat" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("FAT filesystems cannot be shrunk in place".to_string()),
            prerequisites: vec!["Backup data and recreate partition".to_string()],
        },
        "swap" => ResizeInfo {
            can_shrink: true,
            min_size_bytes: Some(0),
            reason: Some("Swap can be recreated at any size".to_string()),
            prerequisites: vec!["Swapoff before modifying".to_string()],
        },
        // Complex/unshrinkable filesystems - keep it simple
        "zfs_member" | "zfs" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("ZFS pools cannot be shrunk".to_string()),
            prerequisites: vec![],
        },
        "LVM2_member" | "lvm" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("LVM requires manual handling".to_string()),
            prerequisites: vec!["Use lvreduce/pvresize for LVM operations".to_string()],
        },
        "crypto_LUKS" | "luks" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("LUKS encryption requires special handling".to_string()),
            prerequisites: vec!["Decrypt and resize filesystem first".to_string()],
        },
        "bcachefs" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("Bcachefs shrinking not supported".to_string()),
            prerequisites: vec![],
        },
        "f2fs" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("F2FS can only grow, not shrink".to_string()),
            prerequisites: vec![],
        },
        "reiserfs" | "reiser4" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("ReiserFS shrinking not recommended".to_string()),
            prerequisites: vec![],
        },
        "jfs" => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("JFS can only grow, not shrink".to_string()),
            prerequisites: vec![],
        },
        _ => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some(format!("Unknown filesystem: {}", fs_type)),
            prerequisites: vec![],
        },
    }
}

/// Get NTFS resize information using ntfsresize
fn get_ntfs_resize_info(device: &str) -> ResizeInfo {
    // Try to get info from ntfsresize
    let output = Command::new("ntfsresize")
        .args(["--info", "--force", device])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Check for common issues
                if stderr.contains("hibernat") || stdout.contains("hibernat") {
                    return ResizeInfo {
                        can_shrink: false,
                        min_size_bytes: None,
                        reason: Some("Windows is hibernated".to_string()),
                        prerequisites: vec![
                            "Boot into Windows".to_string(),
                            "Disable Fast Startup in Power Options".to_string(),
                            "Run: shutdown /s /f /t 0".to_string(),
                        ],
                    };
                }
                if stderr.contains("inconsistent") || stdout.contains("inconsistent") {
                    return ResizeInfo {
                        can_shrink: false,
                        min_size_bytes: None,
                        reason: Some("NTFS filesystem has errors".to_string()),
                        prerequisites: vec![
                            "Boot into Windows".to_string(),
                            "Run: chkdsk /f C:".to_string(),
                        ],
                    };
                }
                return ResizeInfo {
                    can_shrink: false,
                    min_size_bytes: None,
                    reason: Some(format!("ntfsresize failed: {}", stderr.trim())),
                    prerequisites: vec![],
                };
            }

            // Parse minimum size from output
            // Look for "You might resize at XXXXX bytes"
            if let Some(min_bytes) = parse_ntfs_min_size(&stdout) {
                // Add 10% safety margin
                let safe_min = (min_bytes as f64 * 1.1) as u64;
                return ResizeInfo {
                    can_shrink: true,
                    min_size_bytes: Some(safe_min),
                    reason: None,
                    prerequisites: vec![],
                };
            }

            ResizeInfo {
                can_shrink: true,
                min_size_bytes: None,
                reason: Some("Could not determine minimum size".to_string()),
                prerequisites: vec![],
            }
        }
        Err(e) => {
            // ntfsresize not installed or other error
            ResizeInfo {
                can_shrink: false,
                min_size_bytes: None,
                reason: Some(format!("ntfsresize not available: {}", e)),
                prerequisites: vec!["Install ntfs-3g package".to_string()],
            }
        }
    }
}

/// Parse minimum size from ntfsresize output
fn parse_ntfs_min_size(output: &str) -> Option<u64> {
    for line in output.lines() {
        // Look for "You might resize at XXXXX bytes"
        if line.contains("You might resize at") && line.contains("bytes") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if *part == "at"
                    && i + 1 < parts.len()
                    && let Ok(bytes) = parts[i + 1].parse::<u64>()
                {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

/// Get ext2/3/4 resize information using dumpe2fs
fn get_ext_resize_info(device: &str, mount_point: Option<&str>) -> ResizeInfo {
    let output = Command::new("dumpe2fs").args(["-h", device]).output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            let block_size = parse_dumpe2fs_field(&stdout, "Block size:").unwrap_or(4096);
            let block_count = parse_dumpe2fs_field(&stdout, "Block count:").unwrap_or(0);
            let free_blocks = parse_dumpe2fs_field(&stdout, "Free blocks:").unwrap_or(0);

            let used_blocks = block_count.saturating_sub(free_blocks);
            let used_bytes = used_blocks * block_size;
            // Add 20% safety margin
            let min_size = (used_bytes as f64 * 1.2) as u64;

            let mut prerequisites = vec![];
            if mount_point.is_some() {
                prerequisites.push("Unmount filesystem before resizing".to_string());
            }

            ResizeInfo {
                can_shrink: true,
                min_size_bytes: Some(min_size),
                reason: None,
                prerequisites,
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            ResizeInfo {
                can_shrink: false,
                min_size_bytes: None,
                reason: Some(format!("dumpe2fs failed: {}", stderr.trim())),
                prerequisites: vec![],
            }
        }
        Err(e) => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some(format!("dumpe2fs not available: {}", e)),
            prerequisites: vec!["Install e2fsprogs package".to_string()],
        },
    }
}

/// Parse a numeric field from dumpe2fs output
fn parse_dumpe2fs_field(output: &str, field: &str) -> Option<u64> {
    for line in output.lines() {
        if line.starts_with(field) {
            let value_str = line.strip_prefix(field)?.trim();
            return value_str.parse().ok();
        }
    }
    None
}

/// Get Btrfs resize information using btrfs filesystem usage
fn get_btrfs_resize_info(mount_point: Option<&str>) -> ResizeInfo {
    let Some(mp) = mount_point else {
        return ResizeInfo {
            can_shrink: true,
            min_size_bytes: None,
            reason: Some("Btrfs must be mounted to determine min size".to_string()),
            prerequisites: vec!["Mount filesystem to get accurate size info".to_string()],
        };
    };

    let output = Command::new("btrfs")
        .args(["filesystem", "usage", "-b", mp])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse output like:
            // Free (estimated):           123456789 (min: 12345678)
            if let Some(min_bytes) = parse_btrfs_min_free(&stdout) {
                // The "min" is minimum FREE space, so used = total - min_free
                // We need to parse "Device size:" to get total
                if let Some(device_size) = parse_btrfs_device_size(&stdout) {
                    let used_bytes = device_size.saturating_sub(min_bytes);
                    // Add 10% safety margin
                    let min_size = (used_bytes as f64 * 1.1) as u64;
                    return ResizeInfo {
                        can_shrink: true,
                        min_size_bytes: Some(min_size),
                        reason: None,
                        prerequisites: vec![],
                    };
                }
            }

            // Fallback: try to calculate from Used field
            if let Some(used) = parse_btrfs_used(&stdout) {
                // Add 10% safety margin
                let min_size = (used as f64 * 1.1) as u64;
                return ResizeInfo {
                    can_shrink: true,
                    min_size_bytes: Some(min_size),
                    reason: None,
                    prerequisites: vec![],
                };
            }

            ResizeInfo {
                can_shrink: true,
                min_size_bytes: None,
                reason: Some("Could not determine minimum size".to_string()),
                prerequisites: vec![],
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            ResizeInfo {
                can_shrink: true,
                min_size_bytes: None,
                reason: Some(format!("btrfs usage failed: {}", stderr.trim())),
                prerequisites: vec![],
            }
        }
        Err(e) => ResizeInfo {
            can_shrink: true,
            min_size_bytes: None,
            reason: Some(format!("btrfs command not available: {}", e)),
            prerequisites: vec!["Install btrfs-progs package".to_string()],
        },
    }
}

/// Parse "Free (estimated):" line with "(min: XXXXX)" from btrfs filesystem usage
fn parse_btrfs_min_free(output: &str) -> Option<u64> {
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Free (estimated):") {
            // Look for "(min: 123456)"
            if let Some(min_start) = line.find("(min:") {
                let rest = &line[min_start + 5..]; // Skip "(min:"
                let rest = rest.trim_start();
                // Find the closing paren
                if let Some(end) = rest.find(')') {
                    let num_str = rest[..end].trim();
                    return num_str.parse().ok();
                }
            }
        }
    }
    None
}

/// Parse "Device size:" from btrfs filesystem usage
fn parse_btrfs_device_size(output: &str) -> Option<u64> {
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Device size:") {
            // Format: "Device size:                 123456789"
            let value_str = line.strip_prefix("Device size:")?.trim();
            return value_str.parse().ok();
        }
    }
    None
}

/// Parse "Used:" from btrfs filesystem usage output
fn parse_btrfs_used(output: &str) -> Option<u64> {
    for line in output.lines() {
        let line = line.trim();
        // Be careful - there's "Used:" at the top level
        if line.starts_with("Used:") && !line.contains("(") {
            let value_str = line.strip_prefix("Used:")?.trim();
            return value_str.parse().ok();
        }
    }
    None
}

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

    #[test]
    fn test_parse_ntfs_min_size() {
        let output = r#"ntfsresize v2021.8.22 (libntfs-3g)
Device name        : /dev/nvme0n1p3
NTFS volume version: 3.1
Cluster size       : 4096 bytes
Current volume size: 107370311680 bytes (107371 MB)
Current device size: 107374182400 bytes (107375 MB)
Checking filesystem consistency ...
100.00 percent completed
Accounting clusters ...
Space in use       : 46123 MB (43.0%)
Collecting resizing constraints ...
You might resize at 46123456789 bytes or 46124 MB (freeing 61247 MB).
Please make a test run using both the -n and -s options before real resizing!"#;

        assert_eq!(parse_ntfs_min_size(output), Some(46123456789));
    }

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

    #[test]
    fn test_parse_btrfs_min_free() {
        // Sample output from `btrfs filesystem usage -b /mountpoint`
        let btrfs_output = r#"Overall:
    Device size:                  53687091200
    Device allocated:             23756070912
    Device unallocated:           29931020288
    Device missing:                         0
    Device slack:                           0
    Used:                         18556850176
    Free (estimated):             33898004480      (min: 18966994432)
    Free (statfs, currentlyblocks):     33898004480
    Data ratio:                           1.00
    Metadata ratio:                       2.00
    Global reserve:               28475392      (used: 0)
    Multiple profiles:                     no

Data,single: Size:21742223360, Used:17485869056 (80.42%)
   /dev/sda1   21742223360

Metadata,DUP: Size:1006632960, Used:524275712 (52.08%)
   /dev/sda1   2013265920

System,DUP: Size:8388608, Used:16384 (0.20%)
   /dev/sda1   16777216

Unallocated:
   /dev/sda1   29931020288
"#;
        assert_eq!(parse_btrfs_min_free(btrfs_output), Some(18966994432));
    }

    #[test]
    fn test_parse_btrfs_device_size() {
        let btrfs_output = r#"Overall:
    Device size:                  53687091200
    Device allocated:             23756070912
"#;
        assert_eq!(parse_btrfs_device_size(btrfs_output), Some(53687091200));
    }

    #[test]
    fn test_parse_btrfs_used() {
        let btrfs_output = r#"Overall:
    Device size:                  53687091200
    Used:                         18556850176
    Free (estimated):             33898004480      (min: 18966994432)
"#;
        assert_eq!(parse_btrfs_used(btrfs_output), Some(18556850176));
    }

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
