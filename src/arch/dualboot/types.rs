//! Core data structures for dual boot detection

use colored::Colorize;
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

/// Minimum size for a partition to be considered a valid shrink candidate (2 GB)
const MIN_SHRINK_CANDIDATE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

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

    /// Check if dual boot is feasible on this disk
    pub fn check_disk_dualboot_feasibility(&self) -> DualBootFeasibility {
        let feasible_partitions: Vec<String> = self
            .partitions
            .iter()
            .filter(|p| p.is_dualboot_feasible())
            .map(|p| p.device.clone())
            .collect();

        // Check if we have enough unpartitioned space
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

            // Check if there are any partitions at all
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
                // Check if there are shrinkable partitions but not enough space
                let shrinkable: Vec<_> = self
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

    /// Display this disk with its partitions
    pub fn display_disk(&self) {
        // Disk header
        println!(
            "  {} {} {} ({})",
            crate::ui::nerd_font::NerdFont::HardDrive
                .to_string()
                .bright_cyan(),
            self.device.bold(),
            format!("[{}]", self.partition_table).dimmed(),
            self.size_human().bright_white()
        );
        println!("  {}", "─".repeat(60).bright_black());

        if self.partitions.is_empty() {
            println!(
                "    {} {}",
                crate::ui::nerd_font::NerdFont::Bullet.to_string().dimmed(),
                "No partitions".dimmed()
            );
        } else {
            for partition in &self.partitions {
                display_partition_row(partition);
            }
        }
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

    /// Check if a partition is feasible for dual boot installation
    pub fn is_dualboot_feasible(&self) -> bool {
        // Cannot resize EFI partitions
        if self.is_efi {
            return false;
        }

        if !is_supported_auto_resize_fs(self) {
            return false;
        }

        // Must be shrinkable
        let resize_info = match self.resize_info.as_ref() {
            Some(info) => info,
            None => return false,
        };

        let can_shrink = resize_info.can_shrink;

        if !can_shrink {
            return false;
        }

        // Must have enough space
        let min_existing = match resize_info.min_size_bytes {
            Some(min) => min,
            None => return false,
        };

        self.size_bytes.saturating_sub(min_existing) >= crate::arch::dualboot::MIN_LINUX_SIZE
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

/// Display a single partition as a row
fn display_partition_row(partition: &PartitionInfo) {
    let name = partition
        .device
        .strip_prefix("/dev/")
        .unwrap_or(&partition.device);

    let fs_type = partition
        .filesystem
        .as_ref()
        .map(|f| f.fs_type.as_str())
        .unwrap_or("-");

    let type_str = match &partition.partition_type {
        Some(pt) => format!("{} [{}]", fs_type, pt),
        None => fs_type.to_string(),
    };

    let (os_icon, os_text) = if partition.is_efi {
        (
            crate::ui::nerd_font::NerdFont::Efi.to_string(),
            "EFI System Partition".cyan().to_string(),
        )
    } else {
        match &partition.detected_os {
            Some(os) => {
                let icon = match os.os_type {
                    OSType::Windows => crate::ui::nerd_font::NerdFont::Desktop,
                    OSType::Linux => crate::ui::nerd_font::NerdFont::Terminal,
                    OSType::MacOS => crate::ui::nerd_font::NerdFont::Desktop,
                    OSType::Unknown => crate::ui::nerd_font::NerdFont::Question,
                };
                let text = match os.os_type {
                    OSType::Windows => os.name.blue(),
                    OSType::Linux => os.name.green(),
                    OSType::MacOS => os.name.magenta(),
                    OSType::Unknown => os.name.white(),
                };
                (icon.to_string(), text.to_string())
            }
            None => ("".to_string(), "-".dimmed().to_string()),
        }
    };

    let resize_text = match &partition.resize_info {
        Some(info) if partition.is_efi => {
            let reason = info
                .reason
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "Reuse for dual boot".to_string());
            format!(
                "{} {}",
                crate::ui::nerd_font::NerdFont::Check.to_string().green(),
                reason.green()
            )
        }
        Some(info) if info.can_shrink => {
            if let Some(min) = info.min_size_human() {
                format!(
                    "{} min: {}",
                    crate::ui::nerd_font::NerdFont::Check.to_string().green(),
                    min
                )
            } else {
                format!(
                    "{} shrinkable",
                    crate::ui::nerd_font::NerdFont::Check.to_string().green()
                )
            }
        }
        Some(info) => {
            let reason = info
                .reason
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "Not shrinkable".to_string());
            format!(
                "{} {}",
                crate::ui::nerd_font::NerdFont::Cross.to_string().red(),
                reason.dimmed()
            )
        }
        None => "-".dimmed().to_string(),
    };

    println!(
        "    {} {:<14} {:>10}  {:<12}  {} {}",
        crate::ui::nerd_font::NerdFont::Bullet.to_string().dimmed(),
        name,
        partition.size_human().bright_white(),
        type_str.cyan(),
        os_icon,
        os_text
    );

    if let Some(info) = &partition.resize_info {
        if info.can_shrink || info.reason.is_some() {
            println!(
                "      {} {}",
                crate::ui::nerd_font::NerdFont::ArrowSubItem
                    .to_string()
                    .dimmed(),
                resize_text
            );
        }

        if !info.prerequisites.is_empty() {
            for prereq in &info.prerequisites {
                println!(
                    "        {} {}",
                    crate::ui::nerd_font::NerdFont::ArrowPointer
                        .to_string()
                        .dimmed(),
                    prereq.yellow()
                );
            }
        }
    }
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
