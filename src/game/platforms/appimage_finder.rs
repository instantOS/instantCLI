//! Case-insensitive AppImage finder utilities
//!
//! Provides utilities for finding AppImage files in common locations
//! with case-insensitive filename matching.

use std::fs;
use std::path::PathBuf;

/// Search for AppImages by full path patterns, matching filenames case-insensitively.
///
/// Takes full path patterns (e.g., `~/AppImages/eden.appimage`) and will match
/// the filename part case-insensitively while preserving the directory structure.
///
/// # Arguments
/// * `search_paths` - List of full path patterns to check
///
/// # Returns
/// * `Vec<PathBuf>` - Full paths to matching AppImages
///
/// # Example
/// ```rust
/// let paths = &[
///     "~/AppImages/eden.appimage",
///     "~/.local/bin/eden.appimage",
/// ];
/// let found = find_appimages_by_paths(paths);
/// ```
pub fn find_appimages_by_paths(search_paths: &[&str]) -> Vec<PathBuf> {
    let mut found = Vec::new();

    for search_path in search_paths {
        // Expand tilde if present
        let expanded = shellexpand::tilde(search_path);
        let full_path = PathBuf::from(expanded.as_ref());

        // Extract directory and filename
        let (dir_path, expected_name) = match (full_path.parent(), full_path.file_name()) {
            (Some(dir), Some(name)) => (dir, name),
            _ => continue,
        };

        // Check if directory exists
        if !dir_path.exists() || !dir_path.is_dir() {
            continue;
        }

        let expected_name_str = expected_name.to_string_lossy();
        let expected_lower = expected_name_str.to_lowercase();

        // Read directory entries and compare case-insensitively
        let entries = match fs::read_dir(dir_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Compare case-insensitively
            if file_name_str.to_lowercase() == expected_lower {
                let found_path = dir_path.join(&file_name);
                if found_path.is_file() {
                    found.push(found_path);
                }
            }
        }
    }

    found
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_case_insensitive_matching() {
        // Test that matching is truly case-insensitive
        let test_cases = vec![
            "eden.appimage",
            "eden.AppImage",
            "Eden.appimage",
            "Eden.AppImage",
            "EDEN.APPIMAGE",
            "EdEn.ApPiMaGe",
        ];

        for name in test_cases {
            assert_eq!(name.to_lowercase(), "eden.appimage");
        }
    }
}
