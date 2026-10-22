//! File validation utilities for launch command builder

use std::path::Path;

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
        assert!(has_valid_extension(
            Path::new("game.nsp"),
            crate::game::launch_command::EmulatorPlatform::Eden.extensions()
        ));
        assert!(has_valid_extension(
            Path::new("game.XCI"),
            crate::game::launch_command::EmulatorPlatform::Eden.extensions()
        ));
        assert!(!has_valid_extension(
            Path::new("game.iso"),
            crate::game::launch_command::EmulatorPlatform::Eden.extensions()
        ));

        assert!(has_valid_extension(
            Path::new("game.iso"),
            crate::game::launch_command::EmulatorPlatform::Dolphin.extensions()
        ));
        assert!(has_valid_extension(
            Path::new("game.wbfs"),
            crate::game::launch_command::EmulatorPlatform::Dolphin.extensions()
        ));
        assert!(!has_valid_extension(
            Path::new("game.nsp"),
            crate::game::launch_command::EmulatorPlatform::Dolphin.extensions()
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
            crate::game::launch_command::EmulatorPlatform::Azahar.extensions()
        ));
        assert!(has_valid_extension(
            Path::new("game.cia"),
            crate::game::launch_command::EmulatorPlatform::Azahar.extensions()
        ));
        assert!(!has_valid_extension(
            Path::new("game.iso"),
            crate::game::launch_command::EmulatorPlatform::Azahar.extensions()
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
