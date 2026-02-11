//! Eden (Switch emulator) game save auto-discovery
//!
//! Scans Eden's NAND save directories to discover title IDs that have
//! existing save data, then optionally correlates them with ROM files
//! found in Eden's configured game directories and recent file list.
//!
//! The primary discovery source is always the NAND save directory
//! structure — ROM filenames are never relied upon for title ID
//! extraction. ROM association is best-effort: if a ROM file's name
//! happens to contain a known title ID it will be linked, otherwise
//! the save entry is still returned with no ROM path.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// Default Eden data directory
const EDEN_DATA_DIR: &str = "~/.local/share/eden";

/// Default Eden config file
const EDEN_CONFIG_PATH: &str = "~/.config/eden/qt-config.ini";

/// Eden NAND save base path (relative to data dir)
const NAND_SAVE_SUBPATH: &str = "nand/user/save/0000000000000000";

/// Special game directory names that should be skipped when scanning
const SPECIAL_GAMEDIRS: &[&str] = &["SDMC", "UserNAND", "SysNAND"];

/// Valid Switch game file extensions (case-insensitive)
const SWITCH_GAME_EXTENSIONS: &[&str] = &["nsp", "xci"];

/// A discovered Eden game with save data
#[derive(Debug, Clone)]
pub struct EdenDiscoveredGame {
    /// Human-readable display name (filename stem if a ROM was matched,
    /// otherwise the raw title ID)
    pub display_name: String,
    /// 16-character hex title ID
    pub title_id: String,
    /// Path to the ROM file, if one could be associated
    pub game_path: Option<PathBuf>,
    /// Path to the NAND save data directory
    pub save_path: PathBuf,
}

/// Check if the Eden emulator data directory exists
pub fn is_eden_installed() -> bool {
    let expanded = shellexpand::tilde(EDEN_DATA_DIR);
    Path::new(expanded.as_ref()).is_dir()
}

/// Discover Eden games that have existing save data.
///
/// 1. Scans the NAND save directories for title IDs.
/// 2. Collects ROM files from Eden's config (recent files + game dirs).
/// 3. Best-effort matches ROMs to title IDs (by checking whether the
///    filename contains the title ID string).
/// 4. Returns one entry per title ID, sorted by display name.
pub fn discover_eden_games() -> Result<Vec<EdenDiscoveredGame>> {
    if !is_eden_installed() {
        return Ok(Vec::new());
    }

    let data_dir = PathBuf::from(shellexpand::tilde(EDEN_DATA_DIR).into_owned());
    let config_path = PathBuf::from(shellexpand::tilde(EDEN_CONFIG_PATH).into_owned());

    // Step 1: discover all title IDs with save data
    let save_dirs = find_save_directories(&data_dir);
    if save_dirs.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: collect ROM files from config
    let rom_files = collect_rom_files(&config_path);

    // Step 3: try to match ROMs to title IDs
    let rom_index = build_rom_index(&rom_files, &save_dirs);

    // Step 4: build result list
    let mut results: Vec<EdenDiscoveredGame> = save_dirs
        .into_iter()
        .map(|(title_id, save_path)| {
            let (display_name, game_path) = match rom_index.get(&title_id) {
                Some(rom_path) => {
                    let name = display_name_from_path(rom_path);
                    (name, Some(rom_path.clone()))
                }
                None => (title_id.clone(), None),
            };

            EdenDiscoveredGame {
                display_name,
                title_id,
                game_path,
                save_path,
            }
        })
        .collect();

    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });

    Ok(results)
}

// ---------------------------------------------------------------------------
// NAND save directory scanning
// ---------------------------------------------------------------------------

/// Scan Eden's NAND save directories and return a map of title ID → save path.
///
/// Directory structure:
/// `<data_dir>/nand/user/save/0000000000000000/<profile>/<title_id>/`
fn find_save_directories(data_dir: &Path) -> HashMap<String, PathBuf> {
    let mut saves: HashMap<String, PathBuf> = HashMap::new();

    let save_base = data_dir.join(NAND_SAVE_SUBPATH);
    if !save_base.is_dir() {
        return saves;
    }

    let profile_entries = match fs::read_dir(&save_base) {
        Ok(entries) => entries,
        Err(_) => return saves,
    };

    for profile_entry in profile_entries.flatten() {
        let profile_path = profile_entry.path();
        if !profile_path.is_dir() {
            continue;
        }

        let title_entries = match fs::read_dir(&profile_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for title_entry in title_entries.flatten() {
            let title_path = title_entry.path();
            if !title_path.is_dir() {
                continue;
            }

            if let Some(dir_name) = title_entry.file_name().to_str() {
                if is_valid_title_id(dir_name) {
                    let title_id = dir_name.to_uppercase();
                    // First profile wins
                    saves.entry(title_id).or_insert(title_path);
                }
            }
        }
    }

    saves
}

/// Check whether a string looks like a valid 16-character hex title ID
fn is_valid_title_id(s: &str) -> bool {
    s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// ROM file collection (from Eden config)
// ---------------------------------------------------------------------------

/// Collect all ROM file paths from Eden's config (recent files + game dirs).
fn collect_rom_files(config_path: &Path) -> Vec<PathBuf> {
    let config_content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };

    let mut files = Vec::new();

    // Recent files
    for path in parse_recent_files(&config_content) {
        if path.is_file() && is_switch_game_file(&path) {
            files.push(path);
        }
    }

    // Game directories (non-recursive scan)
    for dir in parse_game_directories(&config_content) {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_switch_game_file(&path) {
                    files.push(path);
                }
            }
        }
    }

    files
}

/// Parse the `Paths\recentFiles=` line from Eden's config.
///
/// The value is a comma-space separated list of paths, optionally
/// wrapped in double quotes.
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
                paths.push(PathBuf::from(entry));
            }
        }

        break;
    }

    paths
}

/// Parse `Paths\gamedirs\N\path=` values from Eden's config.
///
/// Skips virtual directory names and non-existent paths.
fn parse_game_directories(config_content: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for line in config_content.lines() {
        let trimmed = line.trim();

        if !trimmed.starts_with("Paths\\gamedirs\\") || !trimmed.contains("\\path=") {
            continue;
        }

        let value = match trimmed.split_once("\\path=") {
            Some((_, v)) => v.trim(),
            None => continue,
        };

        if value.is_empty() || SPECIAL_GAMEDIRS.iter().any(|&s| value == s) {
            continue;
        }

        let dir_path = PathBuf::from(value);
        if dir_path.is_dir() {
            dirs.push(dir_path);
        }
    }

    dirs
}

/// Check if a file has a valid Switch game extension
fn is_switch_game_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let lower = ext.to_lowercase();
            SWITCH_GAME_EXTENSIONS.iter().any(|&valid| lower == valid)
        })
}

// ---------------------------------------------------------------------------
// ROM ↔ title ID matching (best-effort)
// ---------------------------------------------------------------------------

/// Build an index mapping title IDs to ROM paths.
///
/// A ROM is associated with a title ID if its filename (case-insensitive)
/// contains the title ID string. This is a heuristic — filenames are not
/// required to follow any particular convention.
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
///
/// Uses the filename stem and strips any `[...]` bracket groups that
/// some naming conventions include. Falls back to the raw stem.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- title ID validation --

    #[test]
    fn valid_title_id_16_hex() {
        assert!(is_valid_title_id("0123456789ABCDEF"));
        assert!(is_valid_title_id("abcdef0123456789"));
    }

    #[test]
    fn invalid_title_id_wrong_length() {
        assert!(!is_valid_title_id("0123456789ABCDE")); // 15 chars
        assert!(!is_valid_title_id("0123456789ABCDEF0")); // 17 chars
        assert!(!is_valid_title_id(""));
    }

    #[test]
    fn invalid_title_id_non_hex() {
        assert!(!is_valid_title_id("ZZZZZZZZZZZZZZZZ"));
        assert!(!is_valid_title_id("012345678_ABCDEF"));
    }

    // -- bracket stripping --

    #[test]
    fn strip_brackets_removes_groups() {
        assert_eq!(strip_bracket_groups("Name [tag1][tag2]"), "Name");
    }

    #[test]
    fn strip_brackets_preserves_plain_text() {
        assert_eq!(strip_bracket_groups("Plain Name"), "Plain Name");
    }

    #[test]
    fn strip_brackets_nested() {
        assert_eq!(strip_bracket_groups("A [outer [inner]] B"), "A  B");
    }

    #[test]
    fn strip_brackets_empty_result() {
        assert_eq!(strip_bracket_groups("[everything]"), "");
    }

    // -- display name from path --

    #[test]
    fn display_name_strips_brackets_and_extension() {
        let path = PathBuf::from("/games/My Title [AABBCCDD11223344][v0].nsp");
        assert_eq!(display_name_from_path(&path), "My Title");
    }

    #[test]
    fn display_name_plain_filename() {
        let path = PathBuf::from("/roms/cool-game.xci");
        assert_eq!(display_name_from_path(&path), "cool-game");
    }

    #[test]
    fn display_name_all_brackets_falls_back() {
        let path = PathBuf::from("/games/[AABB][v0].nsp");
        // stem is "[AABB][v0]", stripped is empty → fallback to raw stem
        assert_eq!(display_name_from_path(&path), "[AABB][v0]");
    }

    // -- switch game file detection --

    #[test]
    fn switch_game_file_valid_extensions() {
        assert!(is_switch_game_file(Path::new("game.nsp")));
        assert!(is_switch_game_file(Path::new("game.NSP")));
        assert!(is_switch_game_file(Path::new("game.xci")));
        assert!(is_switch_game_file(Path::new("game.XCI")));
    }

    #[test]
    fn switch_game_file_invalid_extensions() {
        assert!(!is_switch_game_file(Path::new("game.iso")));
        assert!(!is_switch_game_file(Path::new("game.txt")));
        assert!(!is_switch_game_file(Path::new("game")));
    }

    // -- config parsing --

    #[test]
    fn parse_recent_files_extracts_paths() {
        let config = concat!(
            "[UI]\n",
            "theme=dark\n",
            "\n",
            "Paths\\recentFiles=\"/mnt/a/one.nsp, /mnt/b/two.xci\"\n",
            "\n",
            "Paths\\gamedirs\\size=1\n",
        );
        let paths = parse_recent_files(config);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/mnt/a/one.nsp"));
        assert_eq!(paths[1], PathBuf::from("/mnt/b/two.xci"));
    }

    #[test]
    fn parse_recent_files_empty_value() {
        let paths = parse_recent_files("Paths\\recentFiles=\"\"");
        assert!(paths.is_empty());
    }

    #[test]
    fn parse_recent_files_missing_key() {
        let paths = parse_recent_files("[UI]\ntheme=dark\n");
        assert!(paths.is_empty());
    }

    #[test]
    fn parse_game_directories_skips_virtual_names() {
        let config = concat!(
            "Paths\\gamedirs\\1\\path=SDMC\n",
            "Paths\\gamedirs\\2\\path=UserNAND\n",
            "Paths\\gamedirs\\3\\path=SysNAND\n",
            "Paths\\gamedirs\\4\\path=/tmp\n",
        );
        let dirs = parse_game_directories(config);
        assert!(dirs.iter().all(|d| {
            let name = d.to_string_lossy();
            !SPECIAL_GAMEDIRS.contains(&name.as_ref())
        }));
    }

    // -- ROM ↔ title ID matching --

    #[test]
    fn rom_index_matches_title_id_in_filename() {
        let tid = "0123456789ABCDEF".to_string();
        let rom = PathBuf::from("/games/My Game [0123456789ABCDEF][v0].nsp");
        let mut saves = HashMap::new();
        saves.insert(
            tid.clone(),
            PathBuf::from("/saves/profile/0123456789ABCDEF"),
        );

        let index = build_rom_index(&[rom.clone()], &saves);
        assert_eq!(index.get(&tid), Some(&rom));
    }

    #[test]
    fn rom_index_no_match_when_id_absent() {
        let tid = "0123456789ABCDEF".to_string();
        let rom = PathBuf::from("/games/some-game.nsp");
        let mut saves = HashMap::new();
        saves.insert(
            tid.clone(),
            PathBuf::from("/saves/profile/0123456789ABCDEF"),
        );

        let index = build_rom_index(&[rom], &saves);
        assert!(index.get(&tid).is_none());
    }
}
