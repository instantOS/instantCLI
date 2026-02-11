//! DuckStation (PlayStation 1 emulator) memory card auto-discovery
//!
//! Scans DuckStation's memcard directories to discover existing memory card files
//! for both Flatpak and native/AppImage installations.
//!
//! Memory cards are typically named with .mcd or .mcr extension and contain
//! save data for PS1 games. The filename may indicate the game.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::DiscoveredGame;
use crate::common::TildePath;
use crate::game::utils::path::tilde_display_string;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// DuckStation Flatpak application ID
const DUCKSTATION_FLATPAK_ID: &str = "org.duckstation.DuckStation";

/// Default memcard paths for different installation types
const MEMCARD_PATHS: &[&str] = &[
    // Flatpak installation
    "~/.var/app/org.duckstation.DuckStation/config/DuckStation/memcards",
    // Native/AppImage installation
    "~/.local/share/duckstation/memcards",
];

/// Memory card file extensions
const MEMCARD_EXTENSIONS: &[&str] = &["mcd", "mcr"];

#[derive(Debug, Clone)]
pub struct DuckstationDiscoveredMemcard {
    pub display_name: String,
    pub memcard_path: PathBuf,
    pub install_type: DuckstationInstallType,
    pub is_existing: bool,
    pub tracked_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuckstationInstallType {
    Flatpak,
    Native,
}

impl std::fmt::Display for DuckstationInstallType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DuckstationInstallType::Flatpak => write!(f, "Flatpak"),
            DuckstationInstallType::Native => write!(f, "Native/AppImage"),
        }
    }
}

impl DuckstationDiscoveredMemcard {
    pub fn new(
        display_name: String,
        memcard_path: PathBuf,
        install_type: DuckstationInstallType,
    ) -> Self {
        Self {
            display_name,
            memcard_path,
            install_type,
            is_existing: false,
            tracked_name: None,
        }
    }

    pub fn existing(memcard: Self, tracked_name: String) -> Self {
        Self {
            is_existing: true,
            tracked_name: Some(tracked_name),
            ..memcard
        }
    }
}

impl DiscoveredGame for DuckstationDiscoveredMemcard {
    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn save_path(&self) -> &PathBuf {
        &self.memcard_path
    }

    fn game_path(&self) -> Option<&PathBuf> {
        None
    }

    fn platform_name(&self) -> &'static str {
        "PlayStation 1"
    }

    fn platform_short(&self) -> &'static str {
        "PS1"
    }

    fn unique_key(&self) -> String {
        format!("duckstation-{}", self.display_name)
    }

    fn is_existing(&self) -> bool {
        self.is_existing
    }

    fn tracked_name(&self) -> Option<&str> {
        self.tracked_name.as_deref()
    }

    fn build_preview(&self) -> FzfPreview {
        let save_display = tilde_display_string(&TildePath::new(self.memcard_path.clone()));
        let header_name = self.tracked_name.as_deref().unwrap_or(&self.display_name);

        let mut builder = PreviewBuilder::new()
            .header(
                if self.is_existing {
                    NerdFont::Check
                } else {
                    NerdFont::Disc
                },
                header_name,
            )
            .text(&format!("Platform: {}", self.platform_name()))
            .text(&format!("Source: {}", self.install_type))
            .blank()
            .separator()
            .blank()
            .text("Memory card:")
            .bullet(&save_display)
            .blank()
            .separator()
            .blank();

        if self.is_existing {
            builder = builder.subtext("Already tracked — press Enter to open game menu");
        } else {
            builder = builder.subtext("Auto-discovered from DuckStation emulator");
        }

        builder.build()
    }

    fn build_launch_command(&self) -> Option<String> {
        get_duckstation_launch_command(self.install_type)
    }

    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

/// Check if DuckStation is installed (either Flatpak or native)
pub fn is_duckstation_installed() -> bool {
    is_flatpak_installed() || is_native_installed()
}

/// Check if DuckStation Flatpak is installed
fn is_flatpak_installed() -> bool {
    let flatpak_path = shellexpand::tilde("~/.var/app/org.duckstation.DuckStation");
    Path::new(flatpak_path.as_ref()).is_dir()
}

/// Check if native DuckStation is installed (has memcards directory)
fn is_native_installed() -> bool {
    let native_path = shellexpand::tilde("~/.local/share/duckstation/memcards");
    Path::new(native_path.as_ref()).is_dir()
}

/// Discover DuckStation memory cards from all available installation types.
///
/// Scans both Flatpak and native memcard directories and returns
/// a combined list of all found memory card files.
pub fn discover_duckstation_memcards() -> Result<Vec<DuckstationDiscoveredMemcard>> {
    let mut results: Vec<DuckstationDiscoveredMemcard> = Vec::new();

    // Scan Flatpak memcards
    let flatpak_path = PathBuf::from(shellexpand::tilde(MEMCARD_PATHS[0]).into_owned());
    if flatpak_path.is_dir() {
        results.extend(scan_memcard_directory(
            &flatpak_path,
            DuckstationInstallType::Flatpak,
        )?);
    }

    // Scan native memcards
    let native_path = PathBuf::from(shellexpand::tilde(MEMCARD_PATHS[1]).into_owned());
    if native_path.is_dir() {
        results.extend(scan_memcard_directory(
            &native_path,
            DuckstationInstallType::Native,
        )?);
    }

    // Sort by display name (case-insensitive)
    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });

    Ok(results)
}

/// Scan a single memcard directory for memory card files
fn scan_memcard_directory(
    dir: &Path,
    install_type: DuckstationInstallType,
) -> Result<Vec<DuckstationDiscoveredMemcard>> {
    let mut memcards: Vec<DuckstationDiscoveredMemcard> = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(memcards),
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Skip directories
        if !path.is_file() {
            continue;
        }

        // Check if it's a memory card file
        if let Some(ext) = path.extension()
            && let Some(ext_str) = ext.to_str()
            && MEMCARD_EXTENSIONS.contains(&ext_str.to_lowercase().as_str())
            && let Some(display_name) = display_name_from_path(&path)
        {
            memcards.push(DuckstationDiscoveredMemcard::new(
                display_name,
                path,
                install_type,
            ));
        }
    }

    Ok(memcards)
}

/// Derive a display name from a memcard file path.
/// Uses the filename stem (without extension).
fn display_name_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

/// Get the appropriate DuckStation launch command for a given installation type.
/// This is used when pre-filling the launch command in the add game flow.
pub fn get_duckstation_launch_command(install_type: DuckstationInstallType) -> Option<String> {
    match install_type {
        DuckstationInstallType::Flatpak => Some(format!("flatpak run {}", DUCKSTATION_FLATPAK_ID)),
        DuckstationInstallType::Native => {
            // Try to find EmuDeck AppImage first
            let emudeck_paths = &["~/emulation/tools/launchers/duckstation.appimage"];
            if let Some(path) =
                crate::game::platforms::appimage_finder::find_appimage_by_paths(emudeck_paths)
            {
                Some(format!("\"{}\"", path.display()))
            } else {
                // Try common AppImage locations
                let appimage_paths = &[
                    "~/AppImages/DuckStation-x64.AppImage",
                    "~/AppImages/duckstation.appimage",
                ];
                if let Some(path) =
                    crate::game::platforms::appimage_finder::find_appimage_by_paths(appimage_paths)
                {
                    Some(format!("\"{}\"", path.display()))
                } else {
                    // Fall back to system command
                    Some("duckstation-qt".to_string())
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_from_path_extracts_stem() {
        let path = PathBuf::from("/memcards/ff7_disc1.mcd");
        assert_eq!(display_name_from_path(&path), Some("ff7_disc1".to_string()));
    }

    #[test]
    fn display_name_from_path_handles_spaces() {
        let path = PathBuf::from("/memcards/Final Fantasy VII.mcr");
        assert_eq!(
            display_name_from_path(&path),
            Some("Final Fantasy VII".to_string())
        );
    }

    #[test]
    fn display_name_from_path_no_extension() {
        let path = PathBuf::from("/memcards/MemoryCard1");
        assert_eq!(
            display_name_from_path(&path),
            Some("MemoryCard1".to_string())
        );
    }

    #[test]
    fn install_type_display_format() {
        assert_eq!(DuckstationInstallType::Flatpak.to_string(), "Flatpak");
        assert_eq!(
            DuckstationInstallType::Native.to_string(),
            "Native/AppImage"
        );
    }
}
