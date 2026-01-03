//! Btrfs resize detection

use crate::arch::dualboot::types::ResizeInfo;
use std::process::Command;

/// Get Btrfs resize information using btrfs filesystem usage
pub fn get_btrfs_resize_info(mount_point: Option<&str>) -> ResizeInfo {
    let Some(mp) = mount_point else {
        return ResizeInfo {
            can_shrink: true,
            min_size_bytes: None,
            reason: Some("Btrfs must be mounted to determine min size".to_string()),
            prerequisites: vec!["Mount filesystem to get accurate size info".to_string()],
        };
    };

    let output = Command::new("btrfs")
        .args(["filesystem", "usage", "-b", mp])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse output like:
            // Free (estimated):           123456789 (min: 12345678)
            if let Some(min_bytes) = parse_btrfs_min_free(&stdout) {
                // The "min" is minimum FREE space, so used = total - min_free
                // We need to parse "Device size:" to get total
                if let Some(device_size) = parse_btrfs_device_size(&stdout) {
                    let used_bytes = device_size.saturating_sub(min_bytes);
                    // Add 10% safety margin
                    let min_size = (used_bytes as f64 * 1.1) as u64;
                    return ResizeInfo {
                        can_shrink: true,
                        min_size_bytes: Some(min_size),
                        reason: None,
                        prerequisites: vec![],
                    };
                }
            }

            // Fallback: try to calculate from Used field
            if let Some(used) = parse_btrfs_used(&stdout) {
                // Add 10% safety margin
                let min_size = (used as f64 * 1.1) as u64;
                return ResizeInfo {
                    can_shrink: true,
                    min_size_bytes: Some(min_size),
                    reason: None,
                    prerequisites: vec![],
                };
            }

            ResizeInfo {
                can_shrink: true,
                min_size_bytes: None,
                reason: Some("Could not determine minimum size".to_string()),
                prerequisites: vec![],
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            ResizeInfo {
                can_shrink: true,
                min_size_bytes: None,
                reason: Some(format!("btrfs usage failed: {}", stderr.trim())),
                prerequisites: vec![],
            }
        }
        Err(e) => ResizeInfo {
            can_shrink: true,
            min_size_bytes: None,
            reason: Some(format!("btrfs command not available: {}", e)),
            prerequisites: vec!["Install btrfs-progs package".to_string()],
        },
    }
}

/// Parse "Free (estimated):" line with "(min: XXXXX)" from btrfs filesystem usage
pub fn parse_btrfs_min_free(output: &str) -> Option<u64> {
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Free (estimated):") {
            // Look for "(min: 123456)"
            if let Some(min_start) = line.find("(min:") {
                let rest = &line[min_start + 5..]; // Skip "(min:"
                let rest = rest.trim_start();
                // Find the closing paren
                if let Some(end) = rest.find(')') {
                    let num_str = rest[..end].trim();
                    return num_str.parse().ok();
                }
            }
        }
    }
    None
}

/// Parse "Device size:" from btrfs filesystem usage
pub fn parse_btrfs_device_size(output: &str) -> Option<u64> {
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Device size:") {
            // Format: "Device size:                 123456789"
            let value_str = line.strip_prefix("Device size:")?.trim();
            return value_str.parse().ok();
        }
    }
    None
}

/// Parse "Used:" from btrfs filesystem usage output
pub fn parse_btrfs_used(output: &str) -> Option<u64> {
    for line in output.lines() {
        let line = line.trim();
        // Be careful - there's "Used:" at the top level
        if line.starts_with("Used:") && !line.contains("(") {
            let value_str = line.strip_prefix("Used:")?.trim();
            return value_str.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_btrfs_min_free() {
        // Sample output from `btrfs filesystem usage -b /mountpoint`
        let btrfs_output = r#"Overall:
    Device size:                  53687091200
    Device allocated:             23756070912
    Device unallocated:           29931020288
    Device missing:                         0
    Device slack:                           0
    Used:                         18556850176
    Free (estimated):             33898004480      (min: 18966994432)
    Free (statfs, currentlyblocks):     33898004480
    Data ratio:                           1.00
    Metadata ratio:                       2.00
    Global reserve:               28475392      (used: 0)
    Multiple profiles:                     no

Data,single: Size:21742223360, Used:17485869056 (80.42%)
   /dev/sda1   21742223360

Metadata,DUP: Size:1006632960, Used:524275712 (52.08%)
   /dev/sda1   2013265920

System,DUP: Size:8388608, Used:16384 (0.20%)
   /dev/sda1   16777216

Unallocated:
   /dev/sda1   29931020288
"#;
        assert_eq!(parse_btrfs_min_free(btrfs_output), Some(18966994432));
    }

    #[test]
    fn test_parse_btrfs_device_size() {
        let btrfs_output = r#"Overall:
    Device size:                  53687091200
    Device allocated:             23756070912
"#;
        assert_eq!(parse_btrfs_device_size(btrfs_output), Some(53687091200));
    }

    #[test]
    fn test_parse_btrfs_used() {
        let btrfs_output = r#"Overall:
    Device size:                  53687091200
    Used:                         18556850176
    Free (estimated):             33898004480      (min: 18966994432)
"#;
        assert_eq!(parse_btrfs_used(btrfs_output), Some(18556850176));
    }
}
