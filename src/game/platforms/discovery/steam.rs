//! Steam Proton prefix auto-discovery
//!
//! Scans Steam `compatdata/*/pfx` prefixes from all configured libraries, maps
//! them to either native Steam app manifests or non-Steam shortcuts, and runs
//! the Ludusavi scanner to resolve actual save paths.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;

use super::DiscoveredGame;
use crate::common::TildePath;
use crate::game::operations::steam::{compute_shortcut_app_id, list_steam_shortcuts};
use crate::game::platforms::ludusavi::{self, DiscoveredWineSave, choose_primary_save};
use crate::game::utils::path::tilde_display_string;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone)]
pub struct SteamDiscoveredGame {
    pub display_name: String,
    pub steam_name: String,
    pub app_id: u32,
    pub prefix_path: PathBuf,
    pub save_path: PathBuf,
    pub launch_command: Option<String>,
    pub is_shortcut: bool,
    pub is_existing: bool,
    pub tracked_name: Option<String>,
}

impl SteamDiscoveredGame {
    fn new(
        display_name: String,
        steam_name: String,
        app_id: u32,
        prefix_path: PathBuf,
        save_path: PathBuf,
        is_shortcut: bool,
    ) -> Self {
        Self {
            display_name,
            steam_name,
            app_id,
            prefix_path,
            save_path,
            launch_command: Some(format!("steam steam://rungameid/{}", app_id)),
            is_shortcut,
            is_existing: false,
            tracked_name: None,
        }
    }
}

impl DiscoveredGame for SteamDiscoveredGame {
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
        "Steam"
    }

    fn platform_short(&self) -> &'static str {
        "Steam"
    }

    fn unique_key(&self) -> String {
        format!("steam:{}|{}", self.app_id, self.save_path.to_string_lossy())
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
                    NerdFont::Steam
                },
                header_name,
            )
            .text("Platform: Steam")
            .text(&format!("App ID: {}", self.app_id))
            .text(&format!(
                "Source: {}",
                if self.is_shortcut {
                    "Non-Steam shortcut"
                } else {
                    "Steam app"
                }
            ))
            .blank()
            .separator()
            .blank()
            .text("Steam title:")
            .bullet(&self.steam_name)
            .blank()
            .text("Proton prefix:")
            .bullet(&prefix_display)
            .blank()
            .text("Save path:")
            .bullet(&save_display);

        if let Some(command) = &self.launch_command {
            builder = builder.blank().text("Launch command:").bullet(command);
        }

        builder = builder.blank().separator().blank();

        if self.is_existing {
            builder = builder.subtext("Already tracked — press Enter to open game menu");
        } else {
            builder = builder.subtext("Auto-discovered from Steam Proton compatdata");
        }

        builder.build()
    }

    fn build_launch_command(&self) -> Option<String> {
        self.launch_command.clone()
    }

    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone)]
struct SteamPrefixCandidate {
    app_id: u32,
    steam_name: String,
    prefix_path: PathBuf,
    is_shortcut: bool,
}

pub fn is_steam_installed() -> bool {
    collect_steam_library_roots().is_ok_and(|roots| !roots.is_empty())
}

pub fn discover_steam_games() -> Result<Vec<SteamDiscoveredGame>> {
    let candidates = collect_steam_prefix_candidates()?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for candidate in candidates {
        let saves = ludusavi::scan_wine_prefix(&candidate.prefix_path).unwrap_or_default();
        let matching_saves: Vec<DiscoveredWineSave> = saves
            .into_iter()
            .filter(|save| {
                save.game_name.trim().is_empty()
                    || names_match(&save.game_name, &candidate.steam_name)
            })
            .collect();

        if let Some(save) = choose_primary_save(matching_saves) {
            results.push(SteamDiscoveredGame::new(
                candidate.steam_name.clone(),
                candidate.steam_name.clone(),
                candidate.app_id,
                candidate.prefix_path.clone(),
                PathBuf::from(save.save_path),
                candidate.is_shortcut,
            ));
        }
    }

    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    results.dedup_by(|a, b| a.unique_key() == b.unique_key());

    Ok(results)
}

fn names_match(ludusavi_name: &str, steam_title: &str) -> bool {
    let ludusavi_lower = ludusavi_name.to_lowercase();
    let steam_lower = steam_title.to_lowercase();

    if ludusavi_lower.contains(&steam_lower) || steam_lower.contains(&ludusavi_lower) {
        return true;
    }

    let normalize = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };

    normalize(ludusavi_name) == normalize(steam_title)
}

fn collect_steam_prefix_candidates() -> Result<Vec<SteamPrefixCandidate>> {
    let libraries = collect_steam_library_roots()?;
    let native_names = collect_native_steam_names(&libraries);
    let shortcut_names = collect_shortcut_names()?;

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for library in libraries {
        let compatdata = library.join("steamapps").join("compatdata");
        if !compatdata.is_dir() {
            continue;
        }

        for entry in fs::read_dir(&compatdata).into_iter().flatten().flatten() {
            let dir_name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            let app_id = match dir_name.parse::<u32>() {
                Ok(app_id) => app_id,
                Err(_) => continue,
            };

            let prefix_path = entry.path().join("pfx");
            if !prefix_path.join("drive_c").is_dir() {
                continue;
            }

            let (steam_name, is_shortcut) = match native_names.get(&app_id) {
                Some(name) => (name.clone(), false),
                None => match shortcut_names.get(&app_id) {
                    Some(name) => (name.clone(), true),
                    None => continue,
                },
            };

            if seen.insert((app_id, prefix_path.clone())) {
                candidates.push(SteamPrefixCandidate {
                    app_id,
                    steam_name,
                    prefix_path,
                    is_shortcut,
                });
            }
        }
    }

    Ok(candidates)
}

fn collect_steam_library_roots() -> Result<Vec<PathBuf>> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let roots = [home.join(".local/share/Steam"), home.join(".steam/steam")];
    let mut libraries = Vec::new();
    let mut seen = HashSet::new();

    for root in roots {
        if !root.exists() {
            continue;
        }

        let steamapps = root.join("steamapps");
        if steamapps.is_dir() {
            insert_canonicalized(&mut libraries, &mut seen, &root);
        }

        let libraryfolders = steamapps.join("libraryfolders.vdf");
        if libraryfolders.is_file() {
            let content = fs::read_to_string(&libraryfolders)
                .with_context(|| format!("Failed to read {}", libraryfolders.display()))?;
            for path in parse_libraryfolders_paths(&content) {
                insert_canonicalized(&mut libraries, &mut seen, &path);
            }
        }
    }

    Ok(libraries)
}

fn insert_canonicalized(target: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: &Path) {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.insert(normalized.clone()) {
        target.push(normalized);
    }
}

fn parse_libraryfolders_paths(content: &str) -> Vec<PathBuf> {
    let path_re = Regex::new(r#""path"\s+"([^"]+)""#).expect("valid regex");
    path_re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| unescape_vdf_path(m.as_str())))
        .map(PathBuf::from)
        .collect()
}

fn unescape_vdf_path(path: &str) -> String {
    path.replace("\\\\", "\\")
}

fn collect_native_steam_names(libraries: &[PathBuf]) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    let appmanifest_re =
        Regex::new(r#"appmanifest_(\d+)\.acf$"#).expect("valid appmanifest filename regex");

    for library in libraries {
        let steamapps = library.join("steamapps");
        let entries = match fs::read_dir(&steamapps) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(caps) = appmanifest_re.captures(file_name) else {
                continue;
            };
            let Ok(app_id) = caps[1].parse::<u32>() else {
                continue;
            };
            if let Some(name) = parse_appmanifest_name(&path) {
                names.entry(app_id).or_insert(name);
            }
        }
    }

    names
}

fn parse_appmanifest_name(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let name_re = Regex::new(r#""name"\s+"([^"]+)""#).expect("valid appmanifest regex");
    name_re
        .captures(&content)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
}

fn collect_shortcut_names() -> Result<HashMap<u32, String>> {
    let shortcuts = list_steam_shortcuts()?;
    let mut names = HashMap::new();
    for shortcut in shortcuts {
        let app_id = compute_shortcut_app_id(&shortcut);
        names.entry(app_id).or_insert(shortcut.app_name);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_libraryfolders_paths() {
        let content = r#"
        "libraryfolders"
        {
            "0"
            {
                "path" "/home/test/.local/share/Steam"
            }
            "1"
            {
                "path" "/mnt/Games/SteamLibrary"
            }
        }
        "#;
        let paths = parse_libraryfolders_paths(content);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[1], PathBuf::from("/mnt/Games/SteamLibrary"));
    }

    #[test]
    fn parses_appmanifest_name() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("appmanifest_123.acf");
        fs::write(
            &manifest,
            "\"AppState\"\n{\n    \"appid\" \"123\"\n    \"name\" \"Test Game\"\n}\n",
        )
        .unwrap();

        assert_eq!(
            parse_appmanifest_name(&manifest).as_deref(),
            Some("Test Game")
        );
    }
}
