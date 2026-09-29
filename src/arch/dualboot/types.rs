//! Core data structures for dual boot detection

use crate::common::format::format_size;
use serde::{Deserialize, Serialize};

/// Minimum ESP size for dual boot (260 MB recommended for multi-OS)
pub const MIN_ESP_SIZE: u64 = 260 * 1024 * 1024;

/// Minimum size for a partition to be considered a valid shrink candidate (2 GB)
const MIN_SHRINK_CANDIDATE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Information about a physical disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Device path (e.g., /dev/nvme0n1)
    pub device: String,
    pub size_bytes: u64,
    /// Partition table type
    pub partition_table: PartitionTableType,
    /// List of partitions on this disk
    pub partitions: Vec<PartitionInfo>,
    /// Largest contiguous unpartitioned space in bytes (detected via sfdisk)
    pub max_contiguous_free_space_bytes: u64,
}

impl DiskInfo {
    pub fn size_human(&self) -> String {
        format_size(self.size_bytes)
    }

    /// Total size claimed by the partition list.
    pub fn partitioned_bytes(&self) -> u64 {
        self.partitions.iter().map(|p| p.size_bytes).sum()
    }

    /// Disk size not covered by any partition.
    ///
    /// Derived rather than stored: it is fully determined by `size_bytes` and
    /// `partitions`, so a stored copy could only ever disagree with them.
    /// Note this is *not* the same as
    /// [`max_contiguous_free_space_bytes`](Self::max_contiguous_free_space_bytes),
    /// which is what the partition table actually leaves free — the two differ
    /// whenever the gaps between partitions are not contiguous.
    pub fn unpartitioned_bytes(&self) -> u64 {
        self.size_bytes.saturating_sub(self.partitioned_bytes())
    }

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

    pub fn check_disk_dualboot_feasibility(&self) -> DualBootFeasibility {
        let feasible_partitions: Vec<String> = self
            .partitions
            .iter()
            .filter(|p| p.is_dualboot_feasible())
            .map(|p| p.device.clone())
            .collect();

        // We check CONTIGUOUS space to ensure we can actually create the partition
        let free_space_bytes = self.max_contiguous_free_space_bytes;
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

            if self.partitions.is_empty() {
                // If empty partitions AND not enough space (checked above), then disk is too small
                DualBootFeasibility {
                    feasible: false,
                    feasible_partitions: vec![],
                    reason: Some(format!(
                        "Disk too small or full (Largest free region: {})",
                        format_size(self.max_contiguous_free_space_bytes)
                    )),
                }
            } else {
                let shrinkable: Vec<_> = self
                    .partitions
                    .iter()
                    .filter(|p| {
                        !p.is_efi
                            && p.resize_info
                                .as_ref()
                                .is_some_and(|r| r.shrinkability.maybe_shrinkable())
                    })
                    .collect();

                // Filter out partitions that are way too small (e.g. < 2GB) to be relevant candidates
                // This prevents misleading messages like "shrinkable partitions found" when only a tiny /boot exists
                let valid_candidates: Vec<_> = shrinkable
                    .iter()
                    .filter(|p| p.size_bytes >= MIN_SHRINK_CANDIDATE_SIZE)
                    .collect();

                if valid_candidates.is_empty() {
                    DualBootFeasibility {
                        feasible: false,
                        feasible_partitions: vec![],
                        reason: Some(
                            "No suitable partitions found (too small or not shrinkable)"
                                .to_string(),
                        ),
                    }
                } else {
                    DualBootFeasibility {
                        feasible: false,
                        feasible_partitions: vec![],
                        reason: Some(format!(
                            "Shrinkable partitions found, but none have enough free space for Linux (need {})",
                            format_size(crate::arch::dualboot::MIN_LINUX_SIZE)
                        )),
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
}

/// Partition table type
#[allow(clippy::upper_case_acronyms)]
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
    pub fn size_human(&self) -> String {
        format_size(self.size_bytes)
    }

    pub fn is_dualboot_feasible(&self) -> bool {
        if self.is_efi {
            return false;
        }

        if !is_supported_auto_resize_fs(self) {
            return false;
        }

        // Must be shrinkable with a known minimum size
        let Some(resize_info) = self.resize_info.as_ref() else {
            return false;
        };
        let Shrinkability::Shrinkable { min_size_bytes } = resize_info.shrinkability else {
            return false;
        };

        self.size_bytes.saturating_sub(min_size_bytes) >= crate::arch::dualboot::MIN_LINUX_SIZE
    }
}

fn is_supported_auto_resize_fs(partition: &PartitionInfo) -> bool {
    matches!(
        partition.filesystem.as_ref().map(|fs| fs.fs_type.as_str()),
        Some("ntfs") | Some("ext4") | Some("ext3") | Some("ext2")
    )
}

/// Filesystem information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemInfo {
    /// Filesystem type (e.g., ntfs, ext4, vfat)
    pub fs_type: crate::common::blockdev::Filesystem,
    pub uuid: Option<String>,
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
    /// Whether and how the partition can be shrunk
    pub shrinkability: Shrinkability,
    /// Prerequisites that must be met before resizing
    pub prerequisites: Vec<String>,
}

/// Whether a partition can be shrunk, and how well its limits are known
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Shrinkability {
    /// Can be shrunk; the minimum remaining size is known
    Shrinkable {
        /// Minimum remaining size in bytes
        min_size_bytes: u64,
    },
    /// Can be shrunk in principle, but the minimum size could not be
    /// determined (e.g. the probing tool failed or must be mounted)
    MinUnknown {
        /// Why the minimum size is unknown
        reason: String,
    },
    /// Cannot be shrunk
    NotShrinkable {
        /// Why the partition cannot be shrunk
        reason: String,
    },
}

impl Shrinkability {
    /// Whether the partition may be shrinkable (known minimum or not)
    pub fn maybe_shrinkable(&self) -> bool {
        !matches!(self, Shrinkability::NotShrinkable { .. })
    }
}

impl ResizeInfo {
    pub fn min_size_human(&self) -> Option<String> {
        match self.shrinkability {
            Shrinkability::Shrinkable { min_size_bytes } => Some(format_size(min_size_bytes)),
            _ => None,
        }
    }

    /// Why the partition is not shrinkable or its minimum size is unknown
    pub fn reason(&self) -> Option<&str> {
        match &self.shrinkability {
            Shrinkability::Shrinkable { .. } => None,
            Shrinkability::MinUnknown { reason } | Shrinkability::NotShrinkable { reason } => {
                Some(reason)
            }
        }
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
    pub feasible: bool,
    pub feasible_partitions: Vec<String>,
    /// Reason why dual boot is not feasible (if applicable)
    pub reason: Option<String>,
}

/// Combined disk information and feasibility analysis
#[derive(Debug, Clone)]
pub struct DiskAnalysis {
    pub disk: DiskInfo,
    pub feasibility: DualBootFeasibility,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_sufficient_free_space_true() {
        let disk = DiskInfo {
            device: "/dev/sda".into(),
            size_bytes: 500 * 1024 * 1024 * 1024,
            partition_table: PartitionTableType::GPT,
            partitions: vec![],
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
            max_contiguous_free_space_bytes: 0,
        };
        let esp = disk.find_reusable_esp().unwrap();
        assert_eq!(esp.device, "/dev/sda2");
    }

    #[test]
    fn test_resize_info_min_size_human() {
        let info = ResizeInfo {
            shrinkability: Shrinkability::Shrinkable {
                min_size_bytes: 10 * 1024 * 1024 * 1024,
            },
            prerequisites: vec![],
        };
        assert_eq!(info.min_size_human(), Some("10.0 GB".to_string()));
        assert!(matches!(
            info.shrinkability,
            Shrinkability::Shrinkable { .. }
        ));
        assert_eq!(info.reason(), None);
    }

    #[test]
    fn test_resize_info_min_size_human_none() {
        let info = ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "Filesystem not supported".into(),
            },
            prerequisites: vec![],
        };
        assert!(info.min_size_human().is_none());
        assert!(!matches!(
            info.shrinkability,
            Shrinkability::Shrinkable { .. }
        ));
        assert_eq!(info.reason(), Some("Filesystem not supported"));
    }

    fn partition(device: &str, size: u64) -> PartitionInfo {
        PartitionInfo {
            device: device.to_string(),
            size_bytes: size,
            filesystem: None,
            detected_os: None,
            resize_info: None,
            mount_point: None,
            is_efi: false,
            partition_type: None,
        }
    }

    fn disk(size: u64, partitions: Vec<PartitionInfo>) -> DiskInfo {
        DiskInfo {
            device: "/dev/vda".to_string(),
            size_bytes: size,
            partition_table: PartitionTableType::GPT,
            partitions,
            max_contiguous_free_space_bytes: 0,
        }
    }

    #[test]
    fn unpartitioned_space_is_derived_from_the_partition_list() {
        let disk = disk(
            1000,
            vec![partition("/dev/vda1", 300), partition("/dev/vda2", 200)],
        );

        assert_eq!(disk.partitioned_bytes(), 500);
        assert_eq!(disk.unpartitioned_bytes(), 500);
    }

    #[test]
    fn a_fully_partitioned_disk_has_none_left() {
        let disk = disk(500, vec![partition("/dev/vda1", 500)]);
        assert_eq!(disk.unpartitioned_bytes(), 0);
    }

    #[test]
    fn unpartitioned_space_saturates_rather_than_underflowing() {
        // Partitions can report more than the disk holds — a corrupt table, or
        // a disk resized underneath us — and that must not wrap to a huge
        // number that reads as "loads of free space".
        let disk = disk(100, vec![partition("/dev/vda1", 900)]);
        assert_eq!(disk.unpartitioned_bytes(), 0);
    }

    #[test]
    fn an_empty_partition_list_leaves_the_whole_disk_unpartitioned() {
        assert_eq!(disk(1000, Vec::new()).unpartitioned_bytes(), 1000);
    }

    #[test]
    fn unpartitioned_space_is_not_the_same_as_contiguous_free_space() {
        // The two answer different questions: one is arithmetic on the partition
        // list, the other is what the partition table actually leaves free. They
        // differ whenever the gaps between partitions are not adjacent, which is
        // the common case on a partitioned disk.
        let mut disk = disk(
            1000,
            vec![partition("/dev/vda1", 300), partition("/dev/vda2", 300)],
        );
        disk.max_contiguous_free_space_bytes = 100;

        assert_eq!(disk.unpartitioned_bytes(), 400);
        assert_eq!(disk.max_contiguous_free_space_bytes, 100);
    }
}
