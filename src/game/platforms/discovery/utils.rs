//! Shared helpers for game save auto-discovery
//!
//! Small, emulator-agnostic utilities used by the individual discovery
//! modules:
//!
//! - Display-name derivation from file paths (with and without
//!   `[...]` bracket-group stripping).
//! - Parsing `Paths\recentFiles=` and `Paths\gamedirs\N\path=` lines
//!   from `qt-config.ini`-style configuration files.
//! - Direct existence checks for emulator install directories.

use std::path::{Path, PathBuf};

/// Derive a display name from a ROM file path.
///
/// Uses the filename stem and strips any `[...]` bracket groups that
/// some naming conventions include. Falls back to the raw stem.
pub(crate) fn display_name_from_path(path: &Path) -> String {
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

/// Derive a display name from a save/memory-card file path.
///
/// Uses the filename stem (without extension) and returns `None` when
/// the stem is not valid UTF-8.
pub(crate) fn display_name_from_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

/// Remove all `[...]` bracket groups from a string and trim whitespace.
pub(crate) fn strip_bracket_groups(s: &str) -> String {
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

/// Parse the `Paths\recentFiles=` line from a `qt-config.ini`-style file.
///
/// The value is a comma-space separated list of paths, optionally
/// wrapped in double quotes. Only the first matching line is used, and
/// no filesystem checks are performed — callers may filter the result
/// by extension afterwards.
pub(crate) fn parse_recent_files(config_content: &str) -> Vec<PathBuf> {
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

/// Parse `Paths\gamedirs\N\path=` values from a `qt-config.ini`-style file.
///
/// Skips virtual directory names in `skip_names` (each emulator lists
/// its own) and non-existent paths.
pub(crate) fn parse_game_directories(config_content: &str, skip_names: &[&str]) -> Vec<PathBuf> {
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

        if value.is_empty() || skip_names.contains(&value) {
            continue;
        }

        let dir_path = PathBuf::from(value);
        if dir_path.is_dir() {
            dirs.push(dir_path);
        }
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // stem is "[AABB][v0]", stripped is empty - fallback to raw stem
        assert_eq!(display_name_from_path(&path), "[AABB][v0]");
    }

    // -- display name from stem --

    #[test]
    fn display_name_stem_extracts_stem() {
        let path = PathBuf::from("/memcards/ff7_disc1.mcd");
        assert_eq!(display_name_from_stem(&path), Some("ff7_disc1".to_string()));
    }

    #[test]
    fn display_name_stem_handles_spaces() {
        let path = PathBuf::from("/memcards/Final Fantasy VII.mcr");
        assert_eq!(
            display_name_from_stem(&path),
            Some("Final Fantasy VII".to_string())
        );
    }

    #[test]
    fn display_name_stem_no_extension() {
        let path = PathBuf::from("/memcards/MemoryCard1");
        assert_eq!(
            display_name_from_stem(&path),
            Some("MemoryCard1".to_string())
        );
    }

    // -- recent files parsing --

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

    // -- game directories parsing --

    #[test]
    fn parse_game_directories_skips_listed_names() {
        let config = concat!(
            "Paths\\gamedirs\\1\\path=SDMC\n",
            "Paths\\gamedirs\\2\\path=UserNAND\n",
            "Paths\\gamedirs\\3\\path=SysNAND\n",
            "Paths\\gamedirs\\4\\path=/tmp\n",
        );
        let dirs = parse_game_directories(config, &["SDMC", "UserNAND", "SysNAND"]);
        assert!(dirs.iter().all(|d| {
            let name = d.to_string_lossy();
            !["SDMC", "UserNAND", "SysNAND"].contains(&name.as_ref())
        }));
        // Note: /tmp exists on most systems, so it may or may not be
        // included depending on the environment; only the skips matter.
    }
}
