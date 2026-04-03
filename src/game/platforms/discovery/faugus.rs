//! Faugus Launcher Wine prefix auto-discovery
//!
//! Scans `~/Faugus/<prefix>/` directories for Wine prefixes managed by
//! Faugus Launcher, then runs the Ludusavi manifest scanner to resolve
//! actual save paths for games installed in each prefix.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::DiscoveredGame;
use crate::common::TildePath;
use crate::game::platforms::ludusavi::scan_primary_wine_prefix_saves;
use crate::game::utils::path::tilde_display_string;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Base directory for Faugus prefixes
const FAUGUS_PREFIXES_DIR: &str = "~/Faugus";

/// A discovered game from a Faugus Wine prefix
#[derive(Debug, Clone)]
pub struct FaugusDiscoveredGame {
    pub display_name: String,
    pub prefix_path: PathBuf,
    pub save_path: PathBuf,
    pub is_existing: bool,
    pub tracked_name: Option<String>,
}

impl FaugusDiscoveredGame {
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

impl DiscoveredGame for FaugusDiscoveredGame {
    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn save_path(&self) -> &PathBuf {
        &self.save_path
    }

    fn game_path(&self) -> Option<&PathBuf> {
        None
    }

    fn prefix_path(&self) -> Option<&PathBuf> {
        Some(&self.prefix_path)
    }

    fn platform_name(&self) -> &'static str {
        "Faugus"
    }

    fn platform_short(&self) -> &'static str {
        "Faugus"
    }

    fn unique_key(&self) -> String {
        format!(
            "faugus:{}|{}",
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
            .text("Platform: Faugus")
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
            builder = builder.subtext("Auto-discovered from Faugus Launcher prefix");
        }

        builder.build()
    }

    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}

/// Check if Faugus Launcher prefixes directory exists
pub fn is_faugus_installed() -> bool {
    let faugus_dir = shellexpand::tilde(FAUGUS_PREFIXES_DIR);
    Path::new(faugus_dir.as_ref()).is_dir()
}

/// Discover games from all Faugus Wine prefixes
pub fn discover_faugus_games() -> Result<Vec<FaugusDiscoveredGame>> {
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

fn scan_prefix(prefix: &Path, prefix_name: &str) -> Vec<FaugusDiscoveredGame> {
    let saves = match scan_primary_wine_prefix_saves(prefix) {
        Ok(saves) => saves,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();

    for save in saves {
        let display_name = if save.game_name.trim().is_empty() {
            format!("Unknown ({})", prefix_name)
        } else {
            save.game_name
        };

        results.push(FaugusDiscoveredGame::new(
            display_name,
            prefix.to_path_buf(),
            PathBuf::from(save.save_path),
        ));
    }

    results
}
