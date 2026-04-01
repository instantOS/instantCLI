//! Generic Wine prefix auto-discovery
//!
//! Scans a bounded set of likely prefix locations and runs the Ludusavi
//! scanner against each valid Wine prefix without recursive walking.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::DiscoveredGame;
use crate::common::TildePath;
use crate::game::platforms::ludusavi::{
    DiscoveredWineSave, choose_primary_save, stream_wine_prefix_games,
};
use crate::game::utils::path::{is_valid_wine_prefix, tilde_display_string};
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone)]
pub struct WineDiscoveredGame {
    pub display_name: String,
    pub prefix_path: PathBuf,
    pub save_path: PathBuf,
    pub is_existing: bool,
    pub tracked_name: Option<String>,
}

impl WineDiscoveredGame {
    pub fn new(display_name: String, prefix_path: PathBuf, save_path: PathBuf) -> Self {
        Self {
            display_name,
            prefix_path,
            save_path,
            is_existing: false,
            tracked_name: None,
        }
    }
}

impl DiscoveredGame for WineDiscoveredGame {
    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn save_path(&self) -> &PathBuf {
        &self.save_path
    }

    fn game_path(&self) -> Option<&PathBuf> {
        Some(&self.prefix_path)
    }

    fn platform_name(&self) -> &'static str {
        "Wine Prefix"
    }

    fn platform_short(&self) -> &'static str {
        "Wine"
    }

    fn unique_key(&self) -> String {
        format!(
            "wine:{}|{}",
            self.prefix_path.to_string_lossy(),
            self.save_path.to_string_lossy()
        )
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
        let prefix_display = tilde_display_string(&TildePath::new(self.prefix_path.clone()));
        let save_display = tilde_display_string(&TildePath::new(self.save_path.clone()));
        let header_name = self.tracked_name.as_deref().unwrap_or(&self.display_name);

        let mut builder = PreviewBuilder::new()
            .header(
                if self.is_existing {
                    NerdFont::Check
                } else {
                    NerdFont::Wine
                },
                header_name,
            )
            .text("Platform: Wine Prefix")
            .blank()
            .separator()
            .blank()
            .text("Prefix:")
            .bullet(&prefix_display)
            .blank()
            .text("Save path:")
            .bullet(&save_display)
            .blank()
            .separator()
            .blank();

        if self.is_existing {
            builder = builder.subtext("Already tracked — press Enter to open game menu");
        } else {
            builder = builder.subtext("Auto-discovered from generic Wine prefix");
        }

        builder.build()
    }

    fn build_launch_command(&self) -> Option<String> {
        None
    }

    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

pub fn is_wine_installed() -> bool {
    dirs::home_dir().is_some_and(|home| {
        home.join(".wine").is_dir()
            || home.join("Games").is_dir()
            || home.join("Games/umu").is_dir()
    })
}

pub fn discover_wine_games() -> Result<Vec<WineDiscoveredGame>> {
    let mut results = Vec::new();
    stream_discover_wine_games(|game| {
        results.push(game);
        Ok(())
    })?;
    Ok(results)
}

pub fn stream_discover_wine_games<F>(mut on_game: F) -> Result<()>
where
    F: FnMut(WineDiscoveredGame) -> Result<()>,
{
    let Some(home) = dirs::home_dir() else {
        return Ok(());
    };

    for prefix in collect_generic_wine_prefixes_from_home(&home) {
        stream_discover_wine_games_in_prefix(&prefix, &mut on_game)?;
    }

    Ok(())
}

pub fn discover_wine_games_in_prefix(prefix: &Path) -> Result<Vec<WineDiscoveredGame>> {
    let mut results = Vec::new();
    stream_discover_wine_games_in_prefix(prefix, |game| {
        results.push(game);
        Ok(())
    })?;
    Ok(results)
}

pub fn stream_discover_wine_games_in_prefix<F>(prefix: &Path, mut on_game: F) -> Result<()>
where
    F: FnMut(WineDiscoveredGame) -> Result<()>,
{
    stream_wine_prefix_games(prefix, |game_saves| {
        if let Some(game) = discovered_game_from_saves(prefix, game_saves) {
            on_game(game)?;
        }
        Ok(())
    })
}

fn discovered_game_from_saves(
    prefix: &Path,
    saves: Vec<DiscoveredWineSave>,
) -> Option<WineDiscoveredGame> {
    let mut grouped: BTreeMap<String, Vec<DiscoveredWineSave>> = BTreeMap::new();

    for save in saves {
        grouped
            .entry(save.game_name.clone())
            .or_default()
            .push(save);
    }

    for (game_name, candidates) in grouped {
        let Some(primary_save) = choose_primary_save(candidates) else {
            continue;
        };

        let display_name = if game_name.trim().is_empty() {
            "Unknown Wine Game".to_string()
        } else {
            game_name
        };

        return Some(WineDiscoveredGame::new(
            display_name,
            prefix.to_path_buf(),
            PathBuf::from(primary_save.save_path),
        ));
    }

    None
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
