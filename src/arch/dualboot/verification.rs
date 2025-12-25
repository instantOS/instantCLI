//! Resize verification utilities for dual boot setup
//!
//! Provides functionality to verify that partition resizing has been completed
//! and there's sufficient space for a new Linux installation.

use anyhow::Result;

use super::MIN_LINUX_SIZE;
use super::detection::{DiskInfo, PartitionInfo, detect_disks, format_size};

/// Result of checking if a resize has been performed
#[derive(Debug, Clone)]
pub struct ResizeStatus {
    /// Whether any resize was detected (partition shrunk or space freed)
    pub resize_detected: bool,
    /// Current partition size in bytes (if partition still exists)
    pub current_partition_size: Option<u64>,
    /// How much space was freed compared to original (0 if none)
    pub space_freed_bytes: u64,
    /// Human-readable message describing the status
    pub message: String,
}

impl ResizeStatus {
    /// Get human-readable current partition size
    pub fn current_partition_human(&self) -> Option<String> {
        self.current_partition_size.map(format_size)
    }

    /// Get human-readable space freed
    pub fn space_freed_human(&self) -> String {
        format_size(self.space_freed_bytes)
    }
}

/// Tracker for verifying partition resize operations
#[derive(Debug, Clone)]
pub struct ResizeVerifier {
    /// Disk device path (e.g., /dev/nvme0n1)
    pub disk_path: String,
    /// Partition device path (e.g., /dev/nvme0n1p2)
    pub partition_path: String,
    /// Original partition size in bytes (before resize)
    pub original_partition_size: u64,
    /// Original unpartitioned space in bytes (before resize)
    pub original_unpartitioned_bytes: u64,
    /// Target partition size after resize (optional)
    pub target_partition_size: Option<u64>,
}

impl ResizeVerifier {
    /// Create a new resize verifier with a target size
    pub fn with_target(
        disk: &DiskInfo,
        partition: &PartitionInfo,
        target_partition_size: u64,
    ) -> Self {
        Self {
            disk_path: disk.device.clone(),
            partition_path: partition.device.clone(),
            original_partition_size: partition.size_bytes,
            original_unpartitioned_bytes: disk.unpartitioned_space_bytes,
            target_partition_size: Some(target_partition_size),
        }
    }

    /// Check if resize has been performed by re-detecting disk state
    ///
    /// This is a synchronous function that should be called from spawn_blocking
    pub fn check(&self) -> Result<ResizeStatus> {
        let disks = detect_disks()?;
        let disk = disks.iter().find(|d| d.device == self.disk_path);

        let Some(disk) = disk else {
            return Ok(ResizeStatus {
                resize_detected: false,
                current_partition_size: None,
                space_freed_bytes: 0,
                message: format!("Disk {} not found", self.disk_path),
            });
        };

        let partition = disk
            .partitions
            .iter()
            .find(|p| p.device == self.partition_path);

        let current_partition_size = partition.map(|p| p.size_bytes);
        let current_unpartitioned = disk.unpartitioned_space_bytes;
        let current_contiguous = disk.max_contiguous_free_space_bytes;

        // Check if resize occurred
        let partition_missing = current_partition_size.is_none();
        let partition_shrunk = current_partition_size
            .map(|s| s < self.original_partition_size)
            .unwrap_or(false);
        let space_freed = current_unpartitioned > self.original_unpartitioned_bytes;
        // Treat a missing partition as a resize event (user removed/repurposed it)
        let resize_detected = partition_missing || partition_shrunk || space_freed;

        let space_freed_bytes =
            current_unpartitioned.saturating_sub(self.original_unpartitioned_bytes);

        // Build message
        let message = if resize_detected {
            if partition_missing {
                format!(
                    "Resize detected! Partition {} is gone. Free space increased by {}. Largest contiguous free space: {}",
                    self.partition_path,
                    format_size(space_freed_bytes),
                    format_size(current_contiguous)
                )
            } else if let Some(current) = current_partition_size {
                format!(
                    "Resize detected! Partition {} is now {} (was {}). Free space increased by {}",
                    self.partition_path,
                    format_size(current),
                    format_size(self.original_partition_size),
                    format_size(space_freed_bytes)
                )
            } else {
                // Fallback, should be covered by partition_missing
                format!(
                    "Resize detected! Free space increased by {}",
                    format_size(space_freed_bytes)
                )
            }
        } else if let Some(current) = current_partition_size {
            let target_info = if let Some(target) = self.target_partition_size {
                format!(" (target: {})", format_size(target))
            } else {
                String::new()
            };
            format!(
                "No resize detected. Partition {} is still {}{}",
                self.partition_path,
                format_size(current),
                target_info
            )
        } else {
            format!("Partition {} not found", self.partition_path)
        };

        Ok(ResizeStatus {
            resize_detected,
            current_partition_size,
            space_freed_bytes,
            message,
        })
    }

    /// Check resize status asynchronously
    pub async fn check_async(&self) -> Result<ResizeStatus> {
        let verifier = self.clone();
        tokio::task::spawn_blocking(move || verifier.check()).await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resize_status_human_readable() {
        let status = ResizeStatus {
            resize_detected: true,
            current_partition_size: Some(50 * 1024 * 1024 * 1024), // 50 GB
            space_freed_bytes: 20 * 1024 * 1024 * 1024,            // 20 GB
            message: "Test".to_string(),
        };

        assert_eq!(
            status.current_partition_human(),
            Some("50.0 GB".to_string())
        );
        assert_eq!(status.space_freed_human(), "20.0 GB".to_string());
    }
}
