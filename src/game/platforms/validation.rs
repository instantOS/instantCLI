//! File validation utilities for launch command builder

use std::path::Path;

/// Valid file extensions for Eden (Switch) games
pub const EDEN_EXTENSIONS: &[&str] = &["nsp", "xci", "nca"];

/// Valid file extensions for Dolphin (GameCube/Wii) games
pub const DOLPHIN_EXTENSIONS: &[&str] = &["iso", "wbfs", "gcm", "ciso", "gcz", "wad", "dol", "elf"];

/// Valid file extensions for PCSX2 (PlayStation 2) games
pub const PCSX2_EXTENSIONS: &[&str] = &["iso", "bin", "chd", "cso", "gz", "elf", "irx"];

/// Valid file extensions for mGBA (Game Boy Advance) games
pub const MGBA_EXTENSIONS: &[&str] = &["gba", "gb", "gbc", "sgb", "zip", "7z"];

/// Valid file extensions for DuckStation (PlayStation 1) games
pub const DUCKSTATION_EXTENSIONS: &[&str] = &[
    "bin", "cue", "iso", "img", "chd", "pbp", "ecm", "mds", "psf", "minipsf", "m3u",
];

/// Valid file extensions for Azahar (3DS) games
pub const AZAHAR_EXTENSIONS: &[&str] = &["3ds", "3dsx", "cia", "app", "elf", "axf", "cci", "cxi"];

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

/// Validate a game file for a given emulator/launcher.
///
/// Checks that the file exists, is a regular file, and has one of the
/// expected extensions for the given emulator.
pub fn validate_game_file(
    path: &Path,
    emulator_name: &str,
    valid_extensions: &[&str],
) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }

    if !has_valid_extension(path, valid_extensions) {
        return Err(format!(
            "Invalid file type for {}. Expected: {}\nGot: {}",
            emulator_name,
            format_valid_extensions(valid_extensions),
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

        assert!(has_valid_extension(
            Path::new("game.3ds"),
            AZAHAR_EXTENSIONS
        ));
        assert!(has_valid_extension(
            Path::new("game.cia"),
            AZAHAR_EXTENSIONS
        ));
        assert!(!has_valid_extension(
            Path::new("game.iso"),
            AZAHAR_EXTENSIONS
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
