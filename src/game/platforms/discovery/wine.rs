//! Generic Wine prefix auto-discovery
//!
//! Scans a bounded set of likely prefix locations and runs the Ludusavi
//! scanner against each valid Wine prefix without recursive walking.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::prefix::{PrefixDiscoveredGame, PrefixSource};
use crate::game::platforms::ludusavi::{
    WinePrefixScanOptions, choose_primary_save, stream_wine_prefix_saves,
};
use crate::game::utils::path::is_valid_wine_prefix;

pub fn is_wine_installed() -> bool {
    dirs::home_dir().is_some_and(|home| {
        home.join(".wine").is_dir()
            || home.join("Games").is_dir()
            || home.join("Games/umu").is_dir()
    })
}

pub fn stream_discover_wine_games<F>(mut on_game: F) -> Result<()>
where
    F: FnMut(PrefixDiscoveredGame) -> Result<()>,
{
    let Some(home) = dirs::home_dir() else {
        return Ok(());
    };

    for prefix in collect_generic_wine_prefixes_from_home(&home) {
        stream_discover_wine_games_in_prefix(&prefix, &mut on_game)?;
    }

    Ok(())
}

pub fn stream_discover_wine_games_in_prefix<F>(prefix: &Path, mut on_game: F) -> Result<()>
where
    F: FnMut(PrefixDiscoveredGame) -> Result<()>,
{
    stream_wine_prefix_saves(WinePrefixScanOptions::new(prefix), |game_saves| {
        if let Some(save) = choose_primary_save(game_saves) {
            let display_name = if save.game_name.trim().is_empty() {
                "Unknown Wine Game".to_string()
            } else {
                save.game_name
            };

            on_game(PrefixDiscoveredGame::new(
                PrefixSource::Wine,
                display_name,
                prefix.to_path_buf(),
                PathBuf::from(save.save_path),
            ))?;
        }
        Ok(())
    })
}

fn collect_generic_wine_prefixes_from_home(home: &Path) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();

    push_if_wine_prefix(&mut prefixes, home.join(".wine"));

    let games_dir = home.join("Games");
    push_if_wine_prefix(&mut prefixes, games_dir.join("umu").join("umu-default"));

    collect_games_prefixes(&mut prefixes, &games_dir);
    collect_umu_prefixes(&mut prefixes, &games_dir.join("umu"));

    prefixes.sort();
    prefixes.dedup();
    prefixes
}

fn collect_games_prefixes(prefixes: &mut Vec<PathBuf>, games_dir: &Path) {
    for entry in read_dir_paths(games_dir) {
        push_if_wine_prefix(prefixes, entry.clone());
        push_if_wine_prefix(prefixes, entry.join("prefix"));
    }
}

fn collect_umu_prefixes(prefixes: &mut Vec<PathBuf>, umu_dir: &Path) {
    for entry in read_dir_paths(umu_dir) {
        push_if_wine_prefix(prefixes, entry);
    }
}

fn read_dir_paths(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .collect()
}

fn push_if_wine_prefix(prefixes: &mut Vec<PathBuf>, candidate: PathBuf) {
    if is_valid_wine_prefix(&candidate) {
        prefixes.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_bounded_generic_prefix_candidates() {
        let home = tempfile::tempdir().unwrap();

        let default_prefix = home.path().join(".wine");
        let games_prefix = home.path().join("Games/custom-prefix");
        let nested_prefix = home.path().join("Games/heroic/prefix");
        let umu_default = home.path().join("Games/umu/umu-default");
        let umu_prefix = home.path().join("Games/umu/game-a");
        let ignored_deep_prefix = home.path().join("Games/outer/inner/prefix");

        for prefix in [
            &default_prefix,
            &games_prefix,
            &nested_prefix,
            &umu_default,
            &umu_prefix,
            &ignored_deep_prefix,
        ] {
            std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
        }

        let prefixes = collect_generic_wine_prefixes_from_home(home.path());

        assert!(prefixes.contains(&default_prefix));
        assert!(prefixes.contains(&games_prefix));
        assert!(prefixes.contains(&nested_prefix));
        assert!(prefixes.contains(&umu_default));
        assert!(prefixes.contains(&umu_prefix));
        assert!(!prefixes.contains(&ignored_deep_prefix));
    }
}
