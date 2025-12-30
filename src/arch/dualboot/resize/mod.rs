//! Partition resize logic for various filesystems

mod ntfs;
mod ext;
mod btrfs;
mod other;

use crate::arch::dualboot::types::ResizeInfo;

// Re-export public functions from submodules
pub use ntfs::{get_ntfs_resize_info, parse_ntfs_min_size};
pub use ext::{get_ext_resize_info, parse_dumpe2fs_field};
pub use btrfs::{get_btrfs_resize_info, parse_btrfs_min_free, parse_btrfs_device_size, parse_btrfs_used};
pub use other::get_other_resize_info;

/// Get resize information for a partition based on filesystem type
pub fn get_resize_info(device: &str, fs_type: &str, mount_point: Option<&str>) -> ResizeInfo {
    match fs_type {
        "ntfs" => get_ntfs_resize_info(device),
        "ext4" | "ext3" | "ext2" => get_ext_resize_info(device, mount_point),
        "btrfs" => get_btrfs_resize_info(mount_point),
        _ => get_other_resize_info(fs_type),
    }
}
