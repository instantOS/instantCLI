//! PCSX2 (PlayStation 2 emulator) memory card auto-discovery
//!
//! Scans PCSX2's memcard directories to discover existing memory card files
//! for both Flatpak and native/AppImage installations.
//!
//! Memory cards are typically named with a .ps2 extension and contain
//! save data for PS2 games. The filename may or may not indicate the game.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::DiscoveredGame;
use crate::common::TildePath;
use crate::game::utils::path::tilde_display_string;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// PCSX2 Flatpak application ID
const PCSX2_FLATPAK_ID: &str = "net.pcsx2.PCSX2";

/// Default memcard paths for different installation types
const MEMCARD_PATHS: &[&str] = &[
    // Flatpak installation
    "~/.var/app/net.pcsx2.PCSX2/config/PCSX2/memcards",
    // Native/AppImage installation
    "~/.config/PCSX2/memcards",
];

/// Memory card file extension
const MEMCARD_EXTENSION: &str = "ps2";

#[derive(Debug, Clone)]
pub struct Pcsx2DiscoveredMemcard {
    pub display_name: String,
    pub memcard_path: PathBuf,
    pub install_type: Pcsx2InstallType,
    pub is_existing: bool,
    pub tracked_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pcsx2InstallType {
    Flatpak,
    Native,
}

impl std::fmt::Display for Pcsx2InstallType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pcsx2InstallType::Flatpak => write!(f, "Flatpak"),
            Pcsx2InstallType::Native => write!(f, "Native/AppImage"),
        }
    }
}

impl Pcsx2DiscoveredMemcard {
    pub fn new(
        display_name: String,
        memcard_path: PathBuf,
        install_type: Pcsx2InstallType,
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

impl DiscoveredGame for Pcsx2DiscoveredMemcard {
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
        "PlayStation 2"
    }

    fn platform_short(&self) -> &'static str {
        "PS2"
    }

    fn unique_key(&self) -> String {
        format!("pcsx2-{}", self.display_name)
    }

    fn is_existing(&self) -> bool {
        self.is_existing
    }

    fn tracked_name(&self) -> Option<&str> {
        self.tracked_name.as_deref()
    }

    fn set_existing(&mut self, tracked_name: String) {
        self.is_existing = true;
        self.tracked_name = Some(tracked_name);
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
            builder = builder.subtext("Auto-discovered from PCSX2 emulator");
        }

        builder.build()
    }

    fn build_launch_command(&self) -> Option<String> {
        get_pcsx2_launch_command(self.install_type)
    }

    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

/// Check if PCSX2 is installed (either Flatpak or native)
pub fn is_pcsx2_installed() -> bool {
    is_flatpak_installed() || is_native_installed()
}

/// Check if PCSX2 Flatpak is installed
fn is_flatpak_installed() -> bool {
    let flatpak_path = shellexpand::tilde("~/.var/app/net.pcsx2.PCSX2");
    Path::new(flatpak_path.as_ref()).is_dir()
}

/// Check if native PCSX2 is installed (has memcards directory)
fn is_native_installed() -> bool {
    let native_path = shellexpand::tilde("~/.config/PCSX2/memcards");
    Path::new(native_path.as_ref()).is_dir()
}

/// Discover PCSX2 memory cards from all available installation types.
///
/// Scans both Flatpak and native memcard directories and returns
/// a combined list of all found memory card files.
pub fn discover_pcsx2_memcards() -> Result<Vec<Pcsx2DiscoveredMemcard>> {
    let mut results: Vec<Pcsx2DiscoveredMemcard> = Vec::new();

    // Scan Flatpak memcards
    let flatpak_path = PathBuf::from(shellexpand::tilde(MEMCARD_PATHS[0]).into_owned());
    if flatpak_path.is_dir() {
        results.extend(scan_memcard_directory(
            &flatpak_path,
            Pcsx2InstallType::Flatpak,
        )?);
    }

    // Scan native memcards
    let native_path = PathBuf::from(shellexpand::tilde(MEMCARD_PATHS[1]).into_owned());
    if native_path.is_dir() {
        results.extend(scan_memcard_directory(
            &native_path,
            Pcsx2InstallType::Native,
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

/// Scan a single memcard directory for .ps2 files
fn scan_memcard_directory(
    dir: &Path,
    install_type: Pcsx2InstallType,
) -> Result<Vec<Pcsx2DiscoveredMemcard>> {
    let mut memcards: Vec<Pcsx2DiscoveredMemcard> = Vec::new();

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

        // Check if it's a .ps2 file
        if let Some(ext) = path.extension()
            && ext.to_str().map(|e| e.to_lowercase()) == Some(MEMCARD_EXTENSION.to_string())
            && let Some(display_name) = display_name_from_path(&path)
        {
            memcards.push(Pcsx2DiscoveredMemcard::new(
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

/// Get the appropriate PCSX2 launch command for a given installation type.
/// This is used when pre-filling the launch command in the add game flow.
pub fn get_pcsx2_launch_command(install_type: Pcsx2InstallType) -> Option<String> {
    match install_type {
        Pcsx2InstallType::Flatpak => Some(format!("flatpak run {}", PCSX2_FLATPAK_ID)),
        Pcsx2InstallType::Native => {
            // Try to find EmuDeck AppImage first
            let emudeck_paths = &["~/emulation/tools/launchers/pcsx2-qt.appimage"];
            if let Some(path) =
                crate::game::platforms::appimage_finder::find_appimage_by_paths(emudeck_paths)
            {
                Some(format!("\"{}\"", path.display()))
            } else {
                // Fall back to system pcsx2 command
                Some("pcsx2-qt".to_string())
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
        let path = PathBuf::from("/memcards/ffx_1.ps2");
        assert_eq!(display_name_from_path(&path), Some("ffx_1".to_string()));
    }

    #[test]
    fn display_name_from_path_handles_spaces() {
        let path = PathBuf::from("/memcards/My Game Save.ps2");
        assert_eq!(
            display_name_from_path(&path),
            Some("My Game Save".to_string())
        );
    }

    #[test]
    fn display_name_from_path_no_extension() {
        let path = PathBuf::from("/memcards/Mcd001");
        assert_eq!(display_name_from_path(&path), Some("Mcd001".to_string()));
    }

    #[test]
    fn install_type_display_format() {
        assert_eq!(Pcsx2InstallType::Flatpak.to_string(), "Flatpak");
        assert_eq!(Pcsx2InstallType::Native.to_string(), "Native/AppImage");
    }
}
