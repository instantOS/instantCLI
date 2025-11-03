use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};

use crate::dot::path_serde::TildePath;
use crate::menu_utils::PathInputSelection;

/// Convert a TildePath to a display string, falling back to absolute path if tilde conversion fails
pub fn tilde_display_string(tilde: &TildePath) -> String {
    tilde
        .to_tilde_string()
        .unwrap_or_else(|_| tilde.as_path().to_string_lossy().to_string())
}

/// Convert a PathInputSelection into a TildePath
/// Returns None if the selection was cancelled or empty
pub fn path_selection_to_tilde(selection: PathInputSelection) -> Result<Option<TildePath>> {
    match selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(
                    TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid path: {e}"))?,
                ))
            }
        }
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
            Ok(Some(TildePath::new(path)))
        }
        PathInputSelection::Cancelled => Ok(None),
    }
}

/// Convert a PathInputSelection into a PathBuf
/// Returns None if the selection was cancelled or empty
pub fn path_selection_to_pathbuf(selection: PathInputSelection) -> Result<Option<PathBuf>> {
    Ok(path_selection_to_tilde(selection)?.map(|tilde| tilde.into_path_buf()))
}

/// Validates that a path is a valid Wine prefix by checking for the presence of a drive_c directory
pub fn is_valid_wine_prefix(path: &Path) -> bool {
    let drive_c_path = path.join("drive_c");
    drive_c_path.exists() && drive_c_path.is_dir()
}

/// Converts a Wine prefix path and a relative path within the prefix to a full path
/// For example, given prefix "/home/user/.wine" and relative path "users/user/AppData/Local/LOA/Saved",
/// this would return "/home/user/.wine/drive_c/users/user/AppData/Local/LOA/Saved"
pub fn wine_prefix_path(prefix: &Path, relative_path: &str) -> PathBuf {
    prefix.join("drive_c").join(relative_path)
}

/// Checks if a path appears to be from a Wine prefix
/// Looks for common Wine directory patterns
pub fn is_wine_prefix_path(path: &str) -> bool {
    // Check for drive_c in the path (case-insensitive for robustness)
    let path_lower = path.to_lowercase();
    if !path_lower.contains("/drive_c/") {
        return false;
    }
    
    // Common Wine directory patterns
    path_lower.contains("/appdata/") 
        || path_lower.contains("/users/") 
        || path_lower.contains("/program files")
}
