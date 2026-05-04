//! lsblk JSON parsing for partition detection

use crate::arch::dualboot::types::*;
use crate::common::blockdev::is_efi_partition_type;
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
    let is_efi = is_efi_partition_type(parttype);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::dualboot::types::ResizeInfo;

    fn noop_resize_info(_size: u64) -> ResizeInfo {
        ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: None,
            prerequisites: vec![],
        }
    }

    fn noop_detect_os(
        _fs: &Option<crate::arch::dualboot::types::FilesystemInfo>,
        _mount: &Option<String>,
    ) -> Option<crate::arch::dualboot::types::DetectedOS> {
        None
    }

    fn make_json(json_str: &str) -> serde_json::Value {
        serde_json::from_str(json_str).unwrap()
    }

    // ── parse_partition ─────────────────────────────────────────────────

    #[test]
    fn test_parse_partition_basic() {
        let json = make_json(
            r#"{"name": "sda1", "size": 1073741824, "fstype": "ext4", "mountpoint": "/"}"#,
        );
        let p = parse_partition(&json, noop_detect_os, noop_resize_info).unwrap();
        assert_eq!(p.device, "/dev/sda1");
        assert_eq!(p.size_bytes, 1073741824);
        assert_eq!(p.filesystem.as_ref().unwrap().fs_type, "ext4");
        assert_eq!(p.mount_point, Some("/".to_string()));
        assert!(!p.is_efi);
    }

    #[test]
    fn test_parse_partition_efi() {
        let json = make_json(
            r#"{"name": "sda1", "size": 536870912, "fstype": "vfat", "mountpoint": "/boot", "parttype": "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"}"#,
        );
        let p = parse_partition(&json, noop_detect_os, noop_resize_info).unwrap();
        assert!(p.is_efi);
        assert_eq!(p.detected_os.as_ref().unwrap().name, "EFI System Partition");
    }

    #[test]
    fn test_parse_partition_no_name_returns_none() {
        let json = make_json(r#"{"size": 1000}"#);
        assert!(parse_partition(&json, noop_detect_os, noop_resize_info).is_none());
    }

    #[test]
    fn test_parse_partition_missing_fields_use_defaults() {
        let json = make_json(r#"{"name": "sda1"}"#);
        let p = parse_partition(&json, noop_detect_os, noop_resize_info).unwrap();
        assert_eq!(p.size_bytes, 0);
        assert!(p.filesystem.is_none());
        assert!(p.mount_point.is_none());
        assert!(!p.is_efi);
        assert!(p.partition_type.is_none());
    }

    #[test]
    fn test_parse_partition_empty_mountpoint_treated_as_none() {
        let json = make_json(r#"{"name": "sda1", "size": 1000, "mountpoint": ""}"#);
        let p = parse_partition(&json, noop_detect_os, noop_resize_info).unwrap();
        assert!(p.mount_point.is_none());
    }

    #[test]
    fn test_parse_partition_empty_fstype_treated_as_none() {
        let json = make_json(r#"{"name": "sda1", "size": 1000, "fstype": ""}"#);
        let p = parse_partition(&json, noop_detect_os, noop_resize_info).unwrap();
        assert!(p.filesystem.is_none());
    }

    #[test]
    fn test_parse_partition_extracts_uuid_and_label() {
        let json = make_json(
            r#"{"name": "sda1", "size": 1000, "fstype": "ext4", "uuid": "abc-123", "label": "root"}"#,
        );
        let p = parse_partition(&json, noop_detect_os, noop_resize_info).unwrap();
        let fs = p.filesystem.as_ref().unwrap();
        assert_eq!(fs.uuid.as_deref(), Some("abc-123"));
        assert_eq!(fs.label.as_deref(), Some("root"));
    }

    #[test]
    fn test_parse_partition_detect_os_called_for_non_efi() {
        let json = make_json(r#"{"name": "sda2", "size": 1000, "fstype": "ntfs"}"#);
        let detect_os = |_fs: &Option<_>, _mount: &Option<String>| {
            Some(crate::arch::dualboot::types::DetectedOS {
                os_type: crate::arch::dualboot::types::OSType::Windows,
                name: "Windows 11".to_string(),
            })
        };
        let p = parse_partition(&json, detect_os, noop_resize_info).unwrap();
        let os = p.detected_os.unwrap();
        assert_eq!(os.os_type, crate::arch::dualboot::types::OSType::Windows);
        assert_eq!(os.name, "Windows 11");
    }

    #[test]
    fn test_parse_partition_efi_skips_detect_os() {
        let json = make_json(
            r#"{"name": "sda1", "size": 1000, "fstype": "vfat", "parttype": "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"}"#,
        );
        // This detect_os would panic if called
        let detect_os = |_fs: &Option<_>, _mount: &Option<String>| {
            panic!("detect_os should not be called for EFI partitions")
        };
        let p = parse_partition(&json, detect_os, noop_resize_info).unwrap();
        assert!(p.is_efi);
        assert_eq!(p.detected_os.as_ref().unwrap().name, "EFI System Partition");
    }

    #[test]
    fn test_parse_partition_efi_uses_resize_fn() {
        let json = make_json(
            r#"{"name": "sda1", "size": 536870912, "fstype": "vfat", "parttype": "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"}"#,
        );
        let resize_fn = |size: u64| ResizeInfo {
            can_shrink: true,
            min_size_bytes: Some(size / 2),
            reason: Some("test".into()),
            prerequisites: vec![],
        };
        let p = parse_partition(&json, noop_detect_os, resize_fn).unwrap();
        let info = p.resize_info.unwrap();
        assert!(info.can_shrink);
        assert_eq!(info.min_size_bytes, Some(536870912 / 2));
    }
}
