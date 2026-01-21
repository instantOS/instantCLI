//! lsblk JSON parsing for partition detection

use crate::arch::dualboot::types::*;
use serde_json::Value;
use std::process::Command;

/// Parse a partition from lsblk JSON
pub fn parse_partition<F, G>(
    value: &Value,
    detect_os_fn: F,
    get_efi_resize_fn: G,
) -> Option<PartitionInfo>
where
    F: Fn(&Option<FilesystemInfo>, &Option<String>) -> Option<DetectedOS>,
    G: Fn(u64) -> ResizeInfo,
{
    let name = value.get("name")?.as_str()?;
    let size_bytes = value.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

    let raw_fs_type = value
        .get("fstype")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let device_path = format!("/dev/{}", name);

    let should_check_bitlocker = raw_fs_type.is_none()
        || raw_fs_type.is_some_and(|fs| {
            fs.eq_ignore_ascii_case("ntfs") || fs.eq_ignore_ascii_case("bitlocker")
        });
    let is_bitlocker = raw_fs_type.is_some_and(|fs| fs.eq_ignore_ascii_case("bitlocker"))
        || (should_check_bitlocker && detect_bitlocker(&device_path));

    let fs_type = if is_bitlocker {
        Some("bitlocker".to_string())
    } else {
        raw_fs_type.map(|fs| fs.to_string())
    };

    let filesystem = fs_type.map(|fs| FilesystemInfo {
        fs_type: fs.to_string(),
        uuid: value
            .get("uuid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        label: value
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    });

    let mount_point = value
        .get("mountpoint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Check if this is an EFI System Partition
    // MBR: 0xef, GPT: C12A7328-F81F-11D2-BA4B-00A0C93EC93B (case insensitive)
    let parttype = value.get("parttype").and_then(|v| v.as_str()).unwrap_or("");
    let is_efi = is_efi_partition(parttype);

    // Detect OS based on filesystem type and mount point
    let detected_os = if is_efi {
        Some(DetectedOS {
            os_type: OSType::Unknown,
            name: "EFI System Partition".to_string(),
        })
    } else {
        detect_os_fn(&filesystem, &mount_point)
    };

    // Get resize info based on filesystem type and EFI status
    // Note: get_resize_info is imported from resize module
    let resize_info = if is_efi {
        Some(get_efi_resize_fn(size_bytes))
    } else {
        filesystem.as_ref().map(|fs| {
            crate::arch::dualboot::resize::get_resize_info(
                &device_path,
                &fs.fs_type,
                mount_point.as_deref(),
            )
        })
    };

    Some(PartitionInfo {
        device: device_path,
        size_bytes,
        filesystem,
        detected_os,
        resize_info,
        mount_point,
        is_efi,
        partition_type: if parttype.is_empty() {
            None
        } else {
            Some(parttype.to_string())
        },
    })
}

/// Check if partition type indicates EFI System Partition
pub fn is_efi_partition(parttype: &str) -> bool {
    let pt = parttype.to_lowercase();
    // MBR type 0xef or ef
    if pt == "0xef" || pt == "ef" {
        return true;
    }
    // GPT GUID for EFI System Partition (case insensitive comparison)
    if pt == "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" {
        return true;
    }
    false
}

fn detect_bitlocker(device_path: &str) -> bool {
    let output = Command::new("blkid")
        .args(["-o", "value", "-s", "TYPE", device_path])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.to_lowercase().contains("bitlocker")
        }
        _ => false,
    }
}
