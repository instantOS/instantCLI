//! Azahar (3DS emulator) game save auto-discovery
//!
//! Scans Azahar's SDMC directories to discover title IDs that have
//! existing save data, then optionally correlates them with ROM files
//! found in Azahar's recent file list.
//!
//! Azahar (Citra fork) stores saves in:
//! `sdmc/Nintendo 3DS/<ID0>/<ID1>/title/<high>/<title_id_low>/data/`
//!
//! Supports both Flatpak and native installations.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// Azahar Flatpak application ID
pub const AZAHAR_FLATPAK_ID: &str = "org.azahar_emu.Azahar";

/// Native Azahar data directory
const NATIVE_DATA_DIR: &str = "~/.local/share/azahar-emu";

/// Native Azahar config directory
const NATIVE_CONFIG_DIR: &str = "~/.config/azahar-emu";

/// Flatpak Azahar data directory
const FLATPAK_DATA_DIR: &str = "~/.var/app/org.azahar_emu.Azahar/data/azahar-emu";

/// Flatpak Azahar config directory
const FLATPAK_CONFIG_DIR: &str = "~/.var/app/org.azahar_emu.Azahar/config/azahar-emu";

/// SDMC subpath (relative to data dir)
const SDMC_SUBPATH: &str = "sdmc/Nintendo 3DS";

/// Valid 3DS game file extensions (case-insensitive)
const AZAHAR_GAME_EXTENSIONS: &[&str] = &["3ds", "3dsx", "cia", "app", "cci", "cxi"];

/// A discovered Azahar game with save data
#[derive(Debug, Clone)]
pub struct AzaharDiscoveredGame {
    /// Human-readable display name (filename stem if a ROM was matched,
    /// otherwise the raw title ID)
    pub display_name: String,
    /// 16-character hex title ID (e.g., "0004000000123400")
    pub title_id: String,
    /// Path to the ROM file, if one could be associated
    pub game_path: Option<PathBuf>,
    /// Path to the save data directory
    pub save_path: PathBuf,
    /// Installation type (flatpak or native)
    pub install_type: AzaharInstallType,
}

/// Installation type for Azahar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AzaharInstallType {
    Flatpak,
    Native,
}

impl std::fmt::Display for AzaharInstallType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AzaharInstallType::Flatpak => write!(f, "Flatpak"),
            AzaharInstallType::Native => write!(f, "Native"),
        }
    }
}

/// Check if Azahar is installed (either Flatpak or native)
pub fn is_azahar_installed() -> bool {
    is_native_installed() || is_flatpak_installed()
}

/// Check if native Azahar is installed
fn is_native_installed() -> bool {
    let native_path = shellexpand::tilde(NATIVE_DATA_DIR);
    Path::new(native_path.as_ref()).is_dir()
}

/// Check if Azahar Flatpak is installed
fn is_flatpak_installed() -> bool {
    let flatpak_path = shellexpand::tilde(FLATPAK_DATA_DIR);
    Path::new(flatpak_path.as_ref()).is_dir()
}

/// Discover Azahar games that have existing save data.
///
/// 1. Scans the SDMC directory structure for title IDs.
/// 2. Collects ROM files from Azahar's recent files config.
/// 3. Best-effort matches ROMs to title IDs.
/// 4. Returns one entry per title ID, sorted by display name.
pub fn discover_azahar_games() -> Result<Vec<AzaharDiscoveredGame>> {
    if !is_azahar_installed() {
        return Ok(Vec::new());
    }

    let mut results: Vec<AzaharDiscoveredGame> = Vec::new();

    if is_native_installed() {
        let native_data = PathBuf::from(shellexpand::tilde(NATIVE_DATA_DIR).into_owned());
        let native_config = PathBuf::from(shellexpand::tilde(NATIVE_CONFIG_DIR).into_owned());
        results.extend(discover_from_installation(
            &native_data,
            &native_config,
            AzaharInstallType::Native,
        )?);
    }

    if is_flatpak_installed() {
        let flatpak_data = PathBuf::from(shellexpand::tilde(FLATPAK_DATA_DIR).into_owned());
        let flatpak_config = PathBuf::from(shellexpand::tilde(FLATPAK_CONFIG_DIR).into_owned());
        results.extend(discover_from_installation(
            &flatpak_data,
            &flatpak_config,
            AzaharInstallType::Flatpak,
        )?);
    }

    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });

    Ok(results)
}

/// Discover games from a specific installation (native or flatpak)
fn discover_from_installation(
    data_dir: &Path,
    config_dir: &Path,
    install_type: AzaharInstallType,
) -> Result<Vec<AzaharDiscoveredGame>> {
    let save_dirs = find_save_directories(data_dir);
    if save_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let rom_files = collect_rom_files(config_dir);
    let rom_index = build_rom_index(&rom_files, &save_dirs);

    let results: Vec<AzaharDiscoveredGame> = save_dirs
        .into_iter()
        .map(|(title_id, save_path)| {
            let (display_name, game_path) = match rom_index.get(&title_id) {
                Some(rom_path) => {
                    let name = display_name_from_path(rom_path);
                    (name, Some(rom_path.clone()))
                }
                None => (title_id.clone(), None),
            };

            AzaharDiscoveredGame {
                display_name,
                title_id,
                game_path,
                save_path,
                install_type,
            }
        })
        .collect();

    Ok(results)
}

/// Get the Azahar launch command for a given installation type.
pub fn get_azahar_launch_command(install_type: AzaharInstallType) -> Option<String> {
    match install_type {
        AzaharInstallType::Flatpak => Some(format!("flatpak run {}", AZAHAR_FLATPAK_ID)),
        AzaharInstallType::Native => Some("azahar".to_string()),
    }
}

/// Scan Azahar's SDMC directory structure and return a map of title ID → save path.
///
/// Directory structure:
/// `<data_dir>/sdmc/Nintendo 3DS/<ID0>/<ID1>/title/<high>/<low>/data/`
fn find_save_directories(data_dir: &Path) -> HashMap<String, PathBuf> {
    let mut saves: HashMap<String, PathBuf> = HashMap::new();

    let sdmc_base = data_dir.join(SDMC_SUBPATH);
    if !sdmc_base.is_dir() {
        return saves;
    }

    let id0_entries = match fs::read_dir(&sdmc_base) {
        Ok(entries) => entries,
        Err(_) => return saves,
    };

    for id0_entry in id0_entries.flatten() {
        let id0_path = id0_entry.path();
        if !id0_path.is_dir() {
            continue;
        }

        let id1_entries = match fs::read_dir(&id0_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for id1_entry in id1_entries.flatten() {
            let id1_path = id1_entry.path();
            if !id1_path.is_dir() {
                continue;
            }

            let title_base = id1_path.join("title");
            if !title_base.is_dir() {
                continue;
            }

            scan_title_directories(&title_base, &mut saves);
        }
    }

    saves
}

/// Scan title directories for save data.
fn scan_title_directories(title_base: &Path, saves: &mut HashMap<String, PathBuf>) {
    let high_entries = match fs::read_dir(title_base) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for high_entry in high_entries.flatten() {
        let high_path = high_entry.path();
        if !high_path.is_dir() {
            continue;
        }

        let low_entries = match fs::read_dir(&high_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for low_entry in low_entries.flatten() {
            let low_path = low_entry.path();
            if !low_path.is_dir() {
                continue;
            }

            let data_path = low_path.join("data");
            if data_path.is_dir() {
                if let (Some(high), Some(low)) = (
                    high_path.file_name().and_then(|n| n.to_str()),
                    low_path.file_name().and_then(|n| n.to_str()),
                ) {
                    if let Some(title_id) = build_title_id(high, low) {
                        saves.entry(title_id).or_insert(data_path);
                    }
                }
            }
        }
    }
}

/// Build a title ID from high and low directory names.
/// High: 8 hex chars, Low: 8 hex chars → Title ID: 16 hex chars
fn build_title_id(high: &str, low: &str) -> Option<String> {
    if high.len() == 8 && low.len() == 8 && is_hex_string(high) && is_hex_string(low) {
        Some(format!("{}{}", high.to_uppercase(), low.to_uppercase()))
    } else {
        None
    }
}

fn is_hex_string(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Collect all ROM file paths from Azahar's config (recent files).
fn collect_rom_files(config_dir: &Path) -> Vec<PathBuf> {
    let qt_config_path = config_dir.join("qt-config.ini");
    let config_content = match fs::read_to_string(&qt_config_path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };

    parse_recent_files(&config_content)
}

/// Parse the `Paths\recentFiles=` line from Azahar's config.
fn parse_recent_files(config_content: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for line in config_content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("Paths\\recentFiles=") {
            continue;
        }

        let value = match trimmed.split_once('=') {
            Some((_, v)) => v.trim(),
            None => continue,
        };

        let value = value.strip_prefix('"').unwrap_or(value);
        let value = value.strip_suffix('"').unwrap_or(value);

        if value.is_empty() {
            continue;
        }

        for entry in value.split(", ") {
            let entry = entry.trim();
            if !entry.is_empty() {
                let path = PathBuf::from(entry);
                if path.is_file() && is_azahar_game_file(&path) {
                    paths.push(path);
                }
            }
        }

        break;
    }

    paths
}

/// Check if a file has a valid 3DS game extension
fn is_azahar_game_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let lower = ext.to_lowercase();
            AZAHAR_GAME_EXTENSIONS.iter().any(|&valid| lower == valid)
        })
}

/// Build an index mapping title IDs to ROM paths.
fn build_rom_index(
    rom_files: &[PathBuf],
    save_dirs: &HashMap<String, PathBuf>,
) -> HashMap<String, PathBuf> {
    let mut index: HashMap<String, PathBuf> = HashMap::new();

    for rom_path in rom_files {
        let file_name = match rom_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_uppercase(),
            None => continue,
        };

        for title_id in save_dirs.keys() {
            if file_name.contains(title_id.as_str()) {
                index
                    .entry(title_id.clone())
                    .or_insert_with(|| rom_path.clone());
            }
        }
    }

    index
}

/// Derive a display name from a ROM file path.
fn display_name_from_path(path: &Path) -> String {
    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return path.to_string_lossy().to_string(),
    };

    let cleaned = strip_bracket_groups(stem);
    if cleaned.is_empty() {
        stem.to_string()
    } else {
        cleaned
    }
}

/// Remove all `[...]` bracket groups from a string and trim whitespace.
fn strip_bracket_groups(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut depth = 0u32;

    for ch in s.chars() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => result.push(ch),
            _ => {}
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_title_id_valid() {
        assert_eq!(
            build_title_id("00040000", "00179800"),
            Some("0004000000179800".to_string())
        );
        assert_eq!(
            build_title_id("00040000", "ABCDEF00"),
            Some("00040000ABCDEF00".to_string())
        );
    }

    #[test]
    fn build_title_id_invalid() {
        assert_eq!(build_title_id("0004000", "00179800"), None);
        assert_eq!(build_title_id("00040000", "0017980"), None);
        assert_eq!(build_title_id("ZZZZZZZZ", "00179800"), None);
    }

    #[test]
    fn strip_brackets_removes_groups() {
        assert_eq!(strip_bracket_groups("Name [tag1][tag2]"), "Name");
    }

    #[test]
    fn strip_brackets_preserves_plain_text() {
        assert_eq!(strip_bracket_groups("Plain Name"), "Plain Name");
    }

    #[test]
    fn is_azahar_game_file_valid() {
        assert!(is_azahar_game_file(Path::new("game.3ds")));
        assert!(is_azahar_game_file(Path::new("game.3DS")));
        assert!(is_azahar_game_file(Path::new("game.cia")));
        assert!(is_azahar_game_file(Path::new("game.cxi")));
    }

    #[test]
    fn is_azahar_game_file_invalid() {
        assert!(!is_azahar_game_file(Path::new("game.iso")));
        assert!(!is_azahar_game_file(Path::new("game.nsp")));
    }
}
