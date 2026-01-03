//! Dual boot feasibility checking

use crate::arch::dualboot::types::format_size;
use crate::arch::dualboot::types::*;

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
