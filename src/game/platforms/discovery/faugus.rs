//! Faugus Launcher Wine prefix auto-discovery
//!
//! Scans `~/Faugus/<prefix>/` directories for Wine prefixes managed by
//! Faugus Launcher, then runs the Ludusavi manifest scanner to resolve
//! actual save paths for games installed in each prefix.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::prefix::{PrefixDiscoveredGame, PrefixSource};
use crate::game::platforms::ludusavi::collect_primary_wine_prefix_saves;

/// Base directory for Faugus prefixes
const FAUGUS_PREFIXES_DIR: &str = "~/Faugus";

/// Check if Faugus Launcher prefixes directory exists
pub fn is_faugus_installed() -> bool {
    let faugus_dir = shellexpand::tilde(FAUGUS_PREFIXES_DIR);
    Path::new(faugus_dir.as_ref()).is_dir()
}

/// Discover games from all Faugus Wine prefixes
pub fn discover_faugus_games() -> Result<Vec<PrefixDiscoveredGame>> {
    let faugus_dir = shellexpand::tilde(FAUGUS_PREFIXES_DIR);
    let faugus_path = Path::new(faugus_dir.as_ref());

    if !faugus_path.is_dir() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();

    for entry in fs::read_dir(faugus_path)? {
        let entry = entry?;
        let prefix_path = entry.path();

        if !prefix_path.is_dir() {
            continue;
        }

        // Must look like a Wine prefix
        if !prefix_path.join("drive_c").is_dir() {
            continue;
        }

        let prefix_name = entry.file_name().to_string_lossy().to_string();
        let prefix_games = scan_prefix(&prefix_path, &prefix_name);
        results.extend(prefix_games);
    }

    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });

    Ok(results)
}

fn scan_prefix(prefix: &Path, prefix_name: &str) -> Vec<PrefixDiscoveredGame> {
    let saves = collect_primary_wine_prefix_saves(prefix);

    let mut results = Vec::new();

    for save in saves {
        let display_name = if save.game_name.trim().is_empty() {
            format!("Unknown ({})", prefix_name)
        } else {
            save.game_name
        };

        results.push(PrefixDiscoveredGame::new(
            PrefixSource::Faugus,
            display_name,
            prefix.to_path_buf(),
            PathBuf::from(save.save_path),
        ));
    }

    results
}
