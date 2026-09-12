//! Resize logic for other filesystems (XFS, FAT, swap, ZFS, LVM, LUKS, etc.)

use crate::arch::dualboot::types::{ResizeInfo, Shrinkability};

/// Get resize information for other filesystems (non-NTFS/ext/Btrfs)
pub fn get_other_resize_info(fs_type: &str) -> ResizeInfo {
    let normalized = fs_type.to_lowercase();

    match normalized.as_str() {
        "bitlocker" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "BitLocker encryption detected".to_string(),
            },
            prerequisites: vec![
                "Decrypt the volume in Windows".to_string(),
                "Disable Fast Startup and hibernation".to_string(),
                "Reboot into Windows once after decrypting".to_string(),
            ],
        },
        "xfs" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "XFS can only grow, not shrink".to_string(),
            },
            prerequisites: vec![],
        },
        "vfat" | "fat32" | "fat16" | "exfat" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "FAT filesystems cannot be shrunk in place".to_string(),
            },
            prerequisites: vec!["Backup data and recreate partition".to_string()],
        },
        "swap" => ResizeInfo {
            shrinkability: Shrinkability::Shrinkable { min_size_bytes: 0 },
            prerequisites: vec!["Swapoff before modifying".to_string()],
        },
        // Complex/unshrinkable filesystems - keep it simple
        "zfs_member" | "zfs" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "ZFS pools cannot be shrunk".to_string(),
            },
            prerequisites: vec![],
        },
        "lvm2_member" | "lvm" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "LVM requires manual handling".to_string(),
            },
            prerequisites: vec!["Use lvreduce/pvresize for LVM operations".to_string()],
        },
        "crypto_luks" | "luks" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "LUKS encryption requires special handling".to_string(),
            },
            prerequisites: vec!["Decrypt and resize filesystem first".to_string()],
        },
        "bcachefs" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "Bcachefs shrinking not supported".to_string(),
            },
            prerequisites: vec![],
        },
        "f2fs" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "F2FS can only grow, not shrink".to_string(),
            },
            prerequisites: vec![],
        },
        "reiserfs" | "reiser4" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "ReiserFS shrinking not recommended".to_string(),
            },
            prerequisites: vec![],
        },
        "jfs" => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: "JFS can only grow, not shrink".to_string(),
            },
            prerequisites: vec![],
        },
        _ => ResizeInfo {
            shrinkability: Shrinkability::NotShrinkable {
                reason: format!("Unknown filesystem: {}", fs_type),
            },
            prerequisites: vec![],
        },
    }
}
