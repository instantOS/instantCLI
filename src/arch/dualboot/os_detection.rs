//! Operating system detection from filesystem info

use crate::arch::dualboot::types::*;

/// Detect OS based on filesystem info (heuristic, no mounting required)
pub fn detect_os_from_info(
    filesystem: &Option<FilesystemInfo>,
    mount_point: &Option<String>,
) -> Option<DetectedOS> {
    let fs = filesystem.as_ref()?;

    match fs.fs_type.as_str() {
        "ntfs" => {
            // NTFS is almost always Windows
            // Check label for hints
            let name = if let Some(label) = &fs.label {
                if label.to_lowercase().contains("windows") {
                    "Windows".to_string()
                } else {
                    "Windows (NTFS)".to_string()
                }
            } else {
                "Windows (NTFS)".to_string()
            };

            Some(DetectedOS {
                os_type: OSType::Windows,
                name,
            })
        }
        "ext4" | "ext3" | "ext2" | "btrfs" | "xfs" => {
            // Linux filesystems
            // Check if it's a root partition
            if mount_point.as_ref().is_some_and(|mp| mp == "/") {
                // Try to read /etc/os-release for the current system
                if let Ok(os_release) = std::fs::read_to_string("/etc/os-release")
                    && let Some(name) = parse_os_release_field(&os_release, "PRETTY_NAME")
                {
                    return Some(DetectedOS {
                        os_type: OSType::Linux,
                        name,
                    });
                }
            }

            Some(DetectedOS {
                os_type: OSType::Linux,
                name: format!("Linux ({})", fs.fs_type),
            })
        }
        "apfs" | "hfsplus" | "hfs" => Some(DetectedOS {
            os_type: OSType::MacOS,
            name: "macOS".to_string(),
        }),
        _ => None,
    }
}

/// Parse a field from /etc/os-release format
pub fn parse_os_release_field(content: &str, field: &str) -> Option<String> {
    for line in content.lines() {
        if line.starts_with(field)
            && let Some(value) = line.strip_prefix(&format!("{}=", field))
        {
            // Remove quotes if present
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_os_release_field() {
        let content = r#"NAME="Arch Linux"
PRETTY_NAME="Arch Linux"
ID=arch
ID_LIKE=archlinux
ANSI_COLOR="38;2;23;147;209"
HOME_URL="https://archlinux.org/"
DOCUMENTATION_URL="https://wiki.archlinux.org/"
"#;

        assert_eq!(
            parse_os_release_field(content, "NAME"),
            Some("Arch Linux".to_string())
        );
        assert_eq!(
            parse_os_release_field(content, "PRETTY_NAME"),
            Some("Arch Linux".to_string())
        );
        assert_eq!(
            parse_os_release_field(content, "ID"),
            Some("arch".to_string())
        );
        assert_eq!(parse_os_release_field(content, "NONEXISTENT"), None);
    }
}
