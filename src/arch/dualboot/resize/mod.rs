//! Partition resize logic for various filesystems

mod btrfs;
mod ext;
mod ntfs;
mod other;

use crate::arch::dualboot::types::ResizeInfo;

// Re-export public functions from submodules
pub use btrfs::get_btrfs_resize_info;
pub use ext::get_ext_resize_info;
pub use ntfs::get_ntfs_resize_info;
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
