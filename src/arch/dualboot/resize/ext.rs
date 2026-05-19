//! ext2/3/4 resize detection

use crate::arch::dualboot::types::ResizeInfo;
use std::process::Command;

/// Get ext2/3/4 resize information using dumpe2fs
pub fn get_ext_resize_info(device: &str, mount_point: Option<&str>) -> ResizeInfo {
    let output = Command::new("dumpe2fs").args(["-h", device]).output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            let block_size = parse_dumpe2fs_field(&stdout, "Block size:").unwrap_or(4096);
            let block_count = parse_dumpe2fs_field(&stdout, "Block count:").unwrap_or(0);
            let free_blocks = parse_dumpe2fs_field(&stdout, "Free blocks:").unwrap_or(0);

            let used_blocks = block_count.saturating_sub(free_blocks);
            let used_bytes = used_blocks * block_size;
            // Add 20% safety margin
            let min_size = (used_bytes as f64 * 1.2) as u64;

            let mut prerequisites = vec![];
            if mount_point.is_some() {
                prerequisites.push("Unmount filesystem before resizing".to_string());
            }

            ResizeInfo {
                can_shrink: true,
                min_size_bytes: Some(min_size),
                reason: None,
                prerequisites,
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            ResizeInfo {
                can_shrink: false,
                min_size_bytes: None,
                reason: Some(format!("dumpe2fs failed: {}", stderr.trim())),
                prerequisites: vec![],
            }
        }
        Err(e) => ResizeInfo {
            can_shrink: false,
            min_size_bytes: None,
            reason: Some(format!("dumpe2fs not available: {}", e)),
            prerequisites: vec!["Install e2fsprogs package".to_string()],
        },
    }
}

/// Parse a numeric field from dumpe2fs output
pub fn parse_dumpe2fs_field(output: &str, field: &str) -> Option<u64> {
    for line in output.lines() {
        if line.starts_with(field) {
            let value_str = line.strip_prefix(field)?.trim();
            return value_str.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::dualboot::test_utils::TestDisk;

    #[test]
    fn test_get_ext_resize_info_e2e() {
        let disk = TestDisk::new(100);
        disk.format_ext4();

        let info = get_ext_resize_info(disk.path_str(), None);
        assert!(info.can_shrink);
        assert!(info.min_size_bytes.is_some());
        assert_eq!(info.prerequisites.len(), 0);
    }

    #[test]
    fn test_get_ext_resize_info_mounted_e2e() {
        let disk = TestDisk::new(100);
        disk.format_ext4();

        // Simulate it being mounted
        let info = get_ext_resize_info(disk.path_str(), Some("/mnt/test"));
        assert!(info.can_shrink);
        assert!(
            info.prerequisites
                .contains(&"Unmount filesystem before resizing".to_string())
        );
    }
}
