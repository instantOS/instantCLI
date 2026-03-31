//! Epic Games (via Legendary/Junkstore) game auto-discovery
//!
//! Scans `legendary list-installed --json` to discover installed Epic Games
//! titles. For each game, detects its Wine prefix and runs the Ludusavi
//! manifest scanner to find accurate save paths instead of using the
//! entire install directory.
//!
//! Junkstore uses legendary under the hood, so this discovery method covers
//! both setups.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde::Deserialize;

use super::DiscoveredGame;
use crate::common::TildePath;
use crate::game::platforms::ludusavi::{self, DiscoveredWineSave, choose_primary_save};
use crate::game::utils::path::tilde_display_string;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// A discovered Epic Games title with an accurate save path from Ludusavi
#[derive(Debug, Clone)]
pub struct EpicDiscoveredGame {
    pub display_name: String,
    pub app_name: String,
    pub install_path: PathBuf,
    pub executable: String,
    pub launch_parameters: String,
    pub save_path: PathBuf,
    pub is_existing: bool,
    pub tracked_name: Option<String>,
}

impl EpicDiscoveredGame {
    pub fn new(
        display_name: String,
        app_name: String,
        install_path: PathBuf,
        executable: String,
        launch_parameters: String,
        save_path: PathBuf,
    ) -> Self {
        Self {
            display_name,
            app_name,
            install_path,
            executable,
            launch_parameters,
            save_path,
            is_existing: false,
            tracked_name: None,
        }
    }

    /// Full path to the game executable
    pub fn exe_path(&self) -> PathBuf {
        self.install_path.join(&self.executable)
    }
}

impl DiscoveredGame for EpicDiscoveredGame {
    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn save_path(&self) -> &PathBuf {
        &self.save_path
    }

    fn game_path(&self) -> Option<&PathBuf> {
        Some(&self.install_path)
    }

    fn platform_name(&self) -> &'static str {
        "Epic Games"
    }

    fn platform_short(&self) -> &'static str {
        "Epic"
    }

    fn unique_key(&self) -> String {
        format!("{}|{}", self.app_name, self.save_path.to_string_lossy())
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
        let install_display = tilde_display_string(&TildePath::new(self.install_path.clone()));
        let save_display = tilde_display_string(&TildePath::new(self.save_path.clone()));
        let exe_path = self.exe_path();
        let exe_display = tilde_display_string(&TildePath::new(exe_path));
        let header_name = self.tracked_name.as_deref().unwrap_or(&self.display_name);

        let mut builder = PreviewBuilder::new()
            .header(
                if self.is_existing {
                    NerdFont::Check
                } else {
                    NerdFont::Windows
                },
                header_name,
            )
            .text(&format!("Platform: {}", self.platform_name()))
            .text(&format!("App ID: {}", self.app_name))
            .blank()
            .separator()
            .blank()
            .text("Install path:")
            .bullet(&install_display)
            .blank()
            .text("Save path:")
            .bullet(&save_display)
            .blank()
            .text("Executable:")
            .bullet(&exe_display);

        if !self.launch_parameters.is_empty() {
            builder = builder
                .blank()
                .text("Launch parameters:")
                .bullet(&self.launch_parameters);
        }

        builder = builder.blank().separator().blank();

        if self.is_existing {
            builder = builder.subtext("Already tracked — press Enter to open game menu");
        } else {
            builder = builder.subtext("Auto-discovered from Epic Games (Legendary/Junkstore)");
        }

        builder.build()
    }

    fn build_launch_command(&self) -> Option<String> {
        // Epic Games are Windows executables. We pre-fill the launch command
        // and let the existing add flow decide whether to wrap it with umu-run.
        let exe = self.exe_path();
        if exe.exists() {
            let exe_str = exe.to_string_lossy();
            let params = if self.launch_parameters.is_empty() {
                String::new()
            } else {
                format!(" {}", self.launch_parameters)
            };
            Some(format!("\"{}\"{}", exe_str, params))
        } else {
            None
        }
    }

    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Deserialize)]
struct LegendaryInstalled {
    app_name: String,
    install_path: PathBuf,
    title: String,
    executable: String,
    #[serde(default)]
    launch_parameters: String,
}

/// Try running legendary (native first, then flatpak) and return JSON output
fn run_legendary_list_installed() -> Option<String> {
    if let Ok(output) = Command::new("legendary")
        .args(["list-installed", "--json"])
        .output()
    {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).into_owned());
        }
    }

    if let Ok(output) = Command::new("flatpak")
        .args([
            "run",
            "com.github.derrod.legendary",
            "list-installed",
            "--json",
        ])
        .output()
    {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).into_owned());
        }
    }

    None
}

/// Check if legendary is available (native or flatpak)
pub fn is_epic_installed() -> bool {
    run_legendary_list_installed().is_some()
}

/// Find the wine prefix root for a given path inside a prefix.
/// Walks up the directory tree looking for a `drive_c` directory.
fn find_wine_prefix(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.join("drive_c").exists() {
            return Some(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return None,
        }
    }
}

/// Check if a Ludusavi game name matches an Epic game title.
/// Uses case-insensitive substring matching in both directions.
fn names_match(ludusavi_name: &str, epic_title: &str) -> bool {
    let ludusavi_lower = ludusavi_name.to_lowercase();
    let epic_lower = epic_title.to_lowercase();

    // Direct substring match either way
    if ludusavi_lower.contains(&epic_lower) || epic_lower.contains(&ludusavi_lower) {
        return true;
    }

    // Normalize: remove common suffixes/punctuation and try again
    let normalize = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ")
            .to_lowercase()
    };

    let ludusavi_norm = normalize(ludusavi_name);
    let epic_norm = normalize(epic_title);

    ludusavi_norm.contains(&epic_norm) || epic_norm.contains(&ludusavi_norm)
}

pub fn stream_discover_epic_games<F>(mut on_game: F) -> Result<()>
where
    F: FnMut(EpicDiscoveredGame) -> Result<()>,
{
    let json_output = match run_legendary_list_installed() {
        Some(output) => output,
        None => return Ok(()),
    };

    let games: Vec<LegendaryInstalled> = match serde_json::from_str(&json_output) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to parse legendary JSON: {e}");
            return Ok(());
        }
    };

    let valid_games: Vec<_> = games
        .into_iter()
        .filter(|g| g.install_path.exists())
        .collect();

    let mut prefix_to_games: HashMap<PathBuf, Vec<&LegendaryInstalled>> = HashMap::new();
    for game in &valid_games {
        if let Some(prefix) = find_wine_prefix(&game.install_path) {
            prefix_to_games.entry(prefix).or_default().push(game);
        }
    }

    let mut prefix_saves: HashMap<PathBuf, Vec<DiscoveredWineSave>> = HashMap::new();
    for prefix in prefix_to_games.keys() {
        if let Ok(saves) = ludusavi::scan_wine_prefix(prefix) {
            prefix_saves.insert(prefix.clone(), saves);
        }
    }

    stream_discover_epic_games_into(valid_games, prefix_saves, &mut on_game)
}

fn stream_discover_epic_games_into<F>(
    valid_games: Vec<LegendaryInstalled>,
    prefix_saves: HashMap<PathBuf, Vec<DiscoveredWineSave>>,
    mut on_game: F,
) -> Result<()>
where
    F: FnMut(EpicDiscoveredGame) -> Result<()>,
{
    for game in &valid_games {
        let prefix = match find_wine_prefix(&game.install_path) {
            Some(p) => p,
            None => {
                // Fallback: use install path as save path (old behavior)
                on_game(EpicDiscoveredGame::new(
                    game.title.clone(),
                    game.app_name.clone(),
                    game.install_path.clone(),
                    game.executable.clone(),
                    game.launch_parameters.clone(),
                    game.install_path.clone(),
                ))?;
                continue;
            }
        };

        let saves = prefix_saves.get(&prefix).cloned().unwrap_or_default();

        // Find matching saves for this game
        let matching_saves: Vec<_> = saves
            .into_iter()
            .filter(|s| names_match(&s.game_name, &game.title))
            .collect();

        if let Some(save) = choose_primary_save(matching_saves) {
            on_game(EpicDiscoveredGame::new(
                save.game_name,
                game.app_name.clone(),
                game.install_path.clone(),
                game.executable.clone(),
                game.launch_parameters.clone(),
                PathBuf::from(save.save_path),
            ))?;
        } else {
            // No Ludusavi match — use install path as fallback
            on_game(EpicDiscoveredGame::new(
                game.title.clone(),
                game.app_name.clone(),
                game.install_path.clone(),
                game.executable.clone(),
                game.launch_parameters.clone(),
                game.install_path.clone(),
            ))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_legendary_json_output() {
        let json = r#"[{
            "app_name": "9b48cbb1a0cf4a73b87ccbf4cde04b26",
            "install_path": "/home/deck/Games/epic/Sable",
            "title": "Sable",
            "version": "4.3.10-R.130",
            "executable": "Sable.exe",
            "launch_parameters": "",
            "platform": "Windows"
        }]"#;

        let games: Vec<LegendaryInstalled> = serde_json::from_str(json).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Sable");
        assert_eq!(games[0].app_name, "9b48cbb1a0cf4a73b87ccbf4cde04b26");
        assert_eq!(games[0].executable, "Sable.exe");
        assert_eq!(
            games[0].install_path,
            PathBuf::from("/home/deck/Games/epic/Sable")
        );
    }

    #[test]
    fn parse_with_launch_parameters() {
        let json = r#"[{
            "app_name": "639c5116ad7d4f31856c34aacda45f2d",
            "install_path": "/run/media/SK512/Games/epic/StyxMasterofShadowshWzSS",
            "title": "Styx: Master of Shadows",
            "executable": "Binaries/Win64/StyxGame.exe",
            "launch_parameters": "-SaveToUserDir"
        }]"#;

        let games: Vec<LegendaryInstalled> = serde_json::from_str(json).unwrap();
        assert_eq!(games[0].launch_parameters, "-SaveToUserDir");
    }

    #[test]
    fn parse_missing_optional_fields() {
        let json = r#"[{
            "app_name": "test123",
            "install_path": "/games/Test",
            "title": "Test Game",
            "executable": "Test.exe"
        }]"#;

        let games: Vec<LegendaryInstalled> = serde_json::from_str(json).unwrap();
        assert_eq!(games[0].launch_parameters, "");
    }

    #[test]
    fn names_match_direct() {
        assert!(names_match("Cyberpunk 2077", "Cyberpunk 2077"));
        assert!(names_match("The Witcher 3", "The Witcher 3: Wild Hunt"));
        assert!(names_match("Hades", "Hades"));
    }

    #[test]
    fn names_match_case_insensitive() {
        assert!(names_match("cyberpunk 2077", "CYBERPUNK 2077"));
        assert!(names_match("HADES", "hades"));
    }

    #[test]
    fn names_match_no_match() {
        assert!(!names_match("Cyberpunk 2077", "The Witcher 3"));
        assert!(!names_match("Hades", "Hollow Knight"));
    }

    #[test]
    fn find_wine_prefix_from_deep_path() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix");
        let drive_c = prefix.join("drive_c");
        let game_dir = drive_c
            .join("Program Files")
            .join("Epic Games")
            .join("Game");
        std::fs::create_dir_all(&game_dir).unwrap();

        let found = find_wine_prefix(&game_dir);
        assert_eq!(found, Some(prefix));
    }

    #[test]
    fn find_wine_prefix_returns_none_for_non_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let game_dir = temp.path().join("native-game");
        std::fs::create_dir_all(&game_dir).unwrap();

        let prefix = find_wine_prefix(&game_dir);
        assert!(prefix.is_none());
    }
}
