//! File validation utilities for launch command builder

use std::path::Path;

/// Valid file extensions for Eden (Switch) games
pub const EDEN_EXTENSIONS: &[&str] = &["nsp", "xci", "nca"];

/// Valid file extensions for Dolphin (GameCube/Wii) games
pub const DOLPHIN_EXTENSIONS: &[&str] = &["iso", "wbfs", "gcm", "ciso", "gcz", "wad", "dol", "elf"];

/// Valid file extensions for Windows executables (umu-run)
pub const WINDOWS_EXTENSIONS: &[&str] = &["exe", "msi", "bat"];

/// Check if a file has a valid extension for a given emulator/launcher
pub fn has_valid_extension(path: &Path, valid_extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_lowercase();
            valid_extensions.iter().any(|valid| *valid == lower)
        })
        .unwrap_or(false)
}

/// Get a human-readable list of valid extensions
pub fn format_valid_extensions(extensions: &[&str]) -> String {
    extensions
        .iter()
        .map(|ext| format!(".{}", ext))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate a file for Eden emulator
pub fn validate_eden_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }

    if !has_valid_extension(path, EDEN_EXTENSIONS) {
        return Err(format!(
            "Invalid file type for Eden. Expected: {}\nGot: {}",
            format_valid_extensions(EDEN_EXTENSIONS),
            path.display()
        ));
    }

    Ok(())
}

/// Validate a file for Dolphin emulator
pub fn validate_dolphin_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }

    if !has_valid_extension(path, DOLPHIN_EXTENSIONS) {
        return Err(format!(
            "Invalid file type for Dolphin. Expected: {}\nGot: {}",
            format_valid_extensions(DOLPHIN_EXTENSIONS),
            path.display()
        ));
    }

    Ok(())
}

/// Validate a file for umu-run (Windows executable)
pub fn validate_windows_executable(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }

    if !has_valid_extension(path, WINDOWS_EXTENSIONS) {
        return Err(format!(
            "Invalid file type for umu-run. Expected: {}\nGot: {}",
            format_valid_extensions(WINDOWS_EXTENSIONS),
            path.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_valid_extension() {
        assert!(has_valid_extension(Path::new("game.nsp"), EDEN_EXTENSIONS));
        assert!(has_valid_extension(Path::new("game.XCI"), EDEN_EXTENSIONS));
        assert!(!has_valid_extension(Path::new("game.iso"), EDEN_EXTENSIONS));

        assert!(has_valid_extension(
            Path::new("game.iso"),
            DOLPHIN_EXTENSIONS
        ));
        assert!(has_valid_extension(
            Path::new("game.wbfs"),
            DOLPHIN_EXTENSIONS
        ));
        assert!(!has_valid_extension(
            Path::new("game.nsp"),
            DOLPHIN_EXTENSIONS
        ));

        assert!(has_valid_extension(
            Path::new("game.exe"),
            WINDOWS_EXTENSIONS
        ));
        assert!(!has_valid_extension(
            Path::new("game.nsp"),
            WINDOWS_EXTENSIONS
        ));
    }

    #[test]
    fn test_format_valid_extensions() {
        assert_eq!(
            format_valid_extensions(&["nsp", "xci"]),
            ".nsp, .xci".to_string()
        );
    }
}
