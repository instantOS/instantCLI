//! Core data structures for dual boot detection

use serde::{Deserialize, Serialize};

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

/// Minimum ESP size for dual boot (260 MB recommended for multi-OS)
pub const MIN_ESP_SIZE: u64 = 260 * 1024 * 1024;

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

/// Combined disk information and feasibility analysis
#[derive(Debug, Clone)]
pub struct DiskAnalysis {
    /// The disk information
    pub disk: DiskInfo,
    /// Dual boot feasibility for this disk
    pub feasibility: DualBootFeasibility,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1000), "1000 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 10), "10.0 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(260 * 1024 * 1024), "260.0 MB");
        assert_eq!(format_size(500 * 1024 * 1024 + 512 * 1024), "500.5 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_size(10 * 1024 * 1024 * 1024), "10.0 GB");
        assert_eq!(format_size(256 * 1024 * 1024 * 1024), "256.0 GB");
    }

    #[test]
    fn test_format_size_tb() {
        assert_eq!(format_size(1024u64 * 1024 * 1024 * 1024), "1.0 TB");
        assert_eq!(format_size(2 * 1024u64 * 1024 * 1024 * 1024), "2.0 TB");
    }

    #[test]
    fn test_has_sufficient_free_space_true() {
        let disk = DiskInfo {
            device: "/dev/sda".into(),
            size_bytes: 500 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![],
            unpartitioned_space_bytes: 0,
            max_contiguous_free_space_bytes: crate::arch::dualboot::MIN_LINUX_SIZE,
        };
        assert!(disk.has_sufficient_free_space());
    }

    #[test]
    fn test_has_sufficient_free_space_false() {
        let disk = DiskInfo {
            device: "/dev/sda".into(),
            size_bytes: 500 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![],
            unpartitioned_space_bytes: 0,
            max_contiguous_free_space_bytes: 1024,
        };
        assert!(!disk.has_sufficient_free_space());
    }

    fn make_esp(size_bytes: u64) -> PartitionInfo {
        PartitionInfo {
            device: "/dev/sda1".into(),
            size_bytes,
            filesystem: Some(FilesystemInfo {
                fs_type: "vfat".into(),
                uuid: None,
                label: None,
            }),
            detected_os: None,
            resize_info: None,
            mount_point: Some("/boot".into()),
            is_efi: true,
            partition_type: Some("C12A7328-F81F-11D2-BA4B-00A0C93EC93B".into()),
        }
    }

    #[test]
    fn test_find_reusable_esp_found() {
        let disk = DiskInfo {
            device: "/dev/sda".into(),
            size_bytes: 500 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![make_esp(MIN_ESP_SIZE)],
            unpartitioned_space_bytes: 0,
            max_contiguous_free_space_bytes: 0,
        };
        let esp = disk.find_reusable_esp().unwrap();
        assert_eq!(esp.size_bytes, MIN_ESP_SIZE);
        assert!(esp.is_efi);
    }

    #[test]
    fn test_find_reusable_esp_too_small() {
        let disk = DiskInfo {
            device: "/dev/sda".into(),
            size_bytes: 500 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![make_esp(100 * 1024 * 1024)], // 100 MB, below 260 MB minimum
            unpartitioned_space_bytes: 0,
            max_contiguous_free_space_bytes: 0,
        };
        assert!(disk.find_reusable_esp().is_none());
    }

    #[test]
    fn test_find_reusable_esp_no_efi_partitions() {
        let non_efi = PartitionInfo {
            device: "/dev/sda1".into(),
            size_bytes: MIN_ESP_SIZE,
            filesystem: None,
            detected_os: None,
            resize_info: None,
            mount_point: None,
            is_efi: false,
            partition_type: None,
        };
        let disk = DiskInfo {
            device: "/dev/sda".into(),
            size_bytes: 500 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![non_efi],
            unpartitioned_space_bytes: 0,
            max_contiguous_free_space_bytes: 0,
        };
        assert!(disk.find_reusable_esp().is_none());
    }

    #[test]
    fn test_find_reusable_esp_first_match_wins() {
        let small_efi = {
            let mut p = make_esp(100 * 1024 * 1024);
            p.device = "/dev/sda1".into();
            p
        };
        let large_efi = {
            let mut p = make_esp(MIN_ESP_SIZE);
            p.device = "/dev/sda2".into();
            p
        };
        let disk = DiskInfo {
            device: "/dev/sda".into(),
            size_bytes: 500 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![small_efi, large_efi],
            unpartitioned_space_bytes: 0,
            max_contiguous_free_space_bytes: 0,
        };
        let esp = disk.find_reusable_esp().unwrap();
        assert_eq!(esp.device, "/dev/sda2");
    }

    #[test]
    fn test_resize_info_min_size_human() {
        let info = ResizeInfo {
            can_shrink: true,
            min_size_bytes: Some(10 * 1024 * 1024 * 1024),
            reason: None,
            prerequisites: vec![],
        };
        assert_eq!(info.min_size_human(), Some("10.0 GB".to_string()));
    }

    #[test]
    fn test_resize_info_min_size_human_none() {
        let info = ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some("Filesystem not supported".into()),
            prerequisites: vec![],
        };
        assert!(info.min_size_human().is_none());
    }
}
