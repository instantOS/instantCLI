//! NTFS resize detection

use crate::arch::dualboot::types::ResizeInfo;
use std::process::Command;

/// Get NTFS resize information using ntfsresize
pub fn get_ntfs_resize_info(device: &str) -> ResizeInfo {
    // Try to get info from ntfsresize
    let output = Command::new("ntfsresize")
        .args(["--info", "--force", device])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Check for common issues
                if stderr.contains("hibernat") || stdout.contains("hibernat") {
                    return ResizeInfo {
                        can_shrink: false,
                        min_size_bytes: None,
                        reason: Some("Windows is hibernated".to_string()),
                        prerequisites: vec![
                            "Boot into Windows".to_string(),
                            "Disable Fast Startup in Power Options".to_string(),
                            "Run: shutdown /s /f /t 0".to_string(),
                        ],
                    };
                }
                if stderr.contains("inconsistent") || stdout.contains("inconsistent") {
                    return ResizeInfo {
                        can_shrink: false,
                        min_size_bytes: None,
                        reason: Some("NTFS filesystem has errors".to_string()),
                        prerequisites: vec![
                            "Boot into Windows".to_string(),
                            "Run: chkdsk /f C:".to_string(),
                        ],
                    };
                }
                return ResizeInfo {
                    can_shrink: false,
                    min_size_bytes: None,
                    reason: Some(format!("ntfsresize failed: {}", stderr.trim())),
                    prerequisites: vec![],
                };
            }

            // Parse minimum size from output
            // Look for "You might resize at XXXXX bytes"
            if let Some(min_bytes) = parse_ntfs_min_size(&stdout) {
                // Add 10% safety margin
                let safe_min = (min_bytes as f64 * 1.1) as u64;
                return ResizeInfo {
                    can_shrink: true,
                    min_size_bytes: Some(safe_min),
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
        Err(e) => {
            // ntfsresize not installed or other error
            ResizeInfo {
                can_shrink: false,
                min_size_bytes: None,
                reason: Some(format!("ntfsresize not available: {}", e)),
                prerequisites: vec!["Install ntfs-3g package".to_string()],
            }
        }
    }
}

/// Parse minimum size from ntfsresize output
pub fn parse_ntfs_min_size(output: &str) -> Option<u64> {
    for line in output.lines() {
        // Look for "You might resize at XXXXX bytes"
        if line.contains("You might resize at") && line.contains("bytes") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if *part == "at"
                    && i + 1 < parts.len()
                    && let Ok(bytes) = parts[i + 1].parse::<u64>()
                {
                    return Some(bytes);
                }
            }
        }
    }
    None
}
