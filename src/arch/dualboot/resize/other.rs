//! Resize logic for other filesystems (XFS, FAT, swap, ZFS, LVM, LUKS, etc.)

use crate::arch::dualboot::types::ResizeInfo;

/// Get resize information for other filesystems (non-NTFS/ext/Btrfs)
pub fn get_other_resize_info(fs_type: &str) -> ResizeInfo {
    match fs_type {
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
