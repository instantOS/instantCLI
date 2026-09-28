//! Partition resize logic for various filesystems

mod btrfs;
mod ext;
mod ntfs;
mod other;
pub mod verification;

use crate::arch::dualboot::types::ResizeInfo;
use crate::common::blockdev::{Filesystem, ShrinkTool};

// Re-export public functions from submodules
pub use btrfs::get_btrfs_resize_info;
pub use ext::get_ext_resize_info;
pub use ntfs::get_ntfs_resize_info;
pub use other::get_other_resize_info;
pub use verification::{ResizeStatus, ResizeVerifier};

/// Get resize information for a partition based on filesystem type
pub fn get_resize_info(
    device: &str,
    fs_type: &Filesystem,
    mount_point: Option<&str>,
) -> ResizeInfo {
    // Dispatch follows the filesystem's own answer, so this table and
    // `Filesystem::shrink_support` cannot disagree about what is supported.
    match fs_type.shrink_support().tool() {
        Some(ShrinkTool::NtfsResize) => get_ntfs_resize_info(device),
        Some(ShrinkTool::Resize2Fs) => get_ext_resize_info(device, mount_point),
        Some(ShrinkTool::Btrfs) => get_btrfs_resize_info(mount_point),
        None => get_other_resize_info(fs_type),
    }
}
