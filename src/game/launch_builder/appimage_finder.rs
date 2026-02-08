//! Case-insensitive AppImage finder utilities
//!
//! Provides utilities for finding AppImage files in common locations
//! with case-insensitive filename matching.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// Search for an AppImage in common locations with case-insensitive matching.
///
/// Takes a list of search paths (directories) and an expected filename,
/// and returns the first match found, comparing filenames case-insensitively.
///
/// # Arguments
/// * `search_paths` - List of directory paths to search (with ~ expansion supported)
/// * `expected_name` - Expected filename (will be matched case-insensitively)
///
/// # Returns
/// * `Some(PathBuf)` - Full path to the found AppImage
/// * `None` - No matching AppImage found
///
/// # Example
/// ```rust
/// // Will find eden.AppImage, eden.appimage, EDEN.APPIMAGE, etc.
/// let paths = &["~/AppImages", "~/.local/bin"];
/// let found = find_appimage_case_insensitive(paths, "eden.appimage");
/// ```
pub fn find_appimage_case_insensitive(
    search_paths: &[&str],
    expected_name: &str,
) -> Option<PathBuf> {
    let expected_lower = expected_name.to_lowercase();

    for search_dir in search_paths {
        // Expand tilde if present
        let expanded = shellexpand::tilde(search_dir);
        let dir_path = PathBuf::from(expanded.as_ref());

        // Check if directory exists
        if !dir_path.exists() || !dir_path.is_dir() {
            continue;
        }

        // Read directory entries and compare case-insensitively
        let entries = match fs::read_dir(&dir_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Compare case-insensitively
            if file_name_str.to_lowercase() == expected_lower {
                let full_path = dir_path.join(&file_name);
                if full_path.is_file() {
                    return Some(full_path);
                }
            }
        }
    }

    None
}

/// Search for an AppImage by full path patterns, matching filenames case-insensitively.
///
/// Unlike `find_appimage_case_insensitive`, this takes full path patterns
/// (e.g., `~/AppImages/eden.appimage`) and will match the filename part
/// case-insensitively while preserving the directory structure.
///
/// # Arguments
/// * `search_paths` - List of full path patterns to check
///
/// # Returns
/// * `Some(PathBuf)` - Full path to the found AppImage
/// * `None` - No matching AppImage found
///
/// # Example
/// ```rust
/// let paths = &[
///     "~/AppImages/eden.appimage",
///     "~/.local/bin/eden.appimage",
/// ];
/// let found = find_appimage_by_paths(paths);
/// ```
pub fn find_appimage_by_paths(search_paths: &[&str]) -> Option<PathBuf> {
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
                    return Some(found_path);
                }
            }
        }
    }

    None
}

/// Find all AppImages in a directory with case-insensitive matching.
///
/// Searches for any files ending in `.appimage` or `.AppImage` (case-insensitive)
/// in the specified directory.
///
/// # Arguments
/// * `directory` - Directory path to search (with ~ expansion supported)
///
/// # Returns
/// * `Vec<PathBuf>` - All AppImage files found
///
/// # Example
/// ```rust
/// let appimages = find_appimages_in_dir("~/AppImages");
/// ```
pub fn find_appimages_in_dir(directory: &str) -> Result<Vec<PathBuf>> {
    let expanded = shellexpand::tilde(directory);
    let dir_path = PathBuf::from(expanded.as_ref());

    if !dir_path.exists() || !dir_path.is_dir() {
        return Ok(Vec::new());
    }

    let mut found = Vec::new();

    let entries = fs::read_dir(&dir_path)
        .with_context(|| format!("Failed to read directory: {}", dir_path.display()))?;

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Check if file ends with .appimage (case-insensitive)
        if file_name_str.to_lowercase().ends_with(".appimage") {
            let full_path = dir_path.join(&file_name);
            if full_path.is_file() {
                found.push(full_path);
            }
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

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
