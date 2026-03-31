//! Epic Games (via Legendary/Junkstore) game auto-discovery
//!
//! Scans `legendary list-installed --json` to discover installed Epic Games
//! titles. Works with both native legendary and the Flatpak version.
//!
//! Junkstore uses legendary under the hood, so this discovery method covers
//! both setups.

use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use serde::Deserialize;

use super::DiscoveredGame;
use crate::common::TildePath;
use crate::game::utils::path::tilde_display_string;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// A discovered Epic Games title
#[derive(Debug, Clone)]
pub struct EpicDiscoveredGame {
    pub display_name: String,
    pub app_name: String,
    pub install_path: PathBuf,
    pub executable: String,
    pub launch_parameters: String,
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
    ) -> Self {
        Self {
            display_name,
            app_name,
            install_path,
            executable,
            launch_parameters,
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
        &self.install_path
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
        self.app_name.clone()
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

/// Discover installed Epic Games via legendary
pub fn discover_epic_games() -> Result<Vec<EpicDiscoveredGame>> {
    let json_output = match run_legendary_list_installed() {
        Some(output) => output,
        None => return Ok(Vec::new()),
    };

    let games: Vec<LegendaryInstalled> = match serde_json::from_str(&json_output) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to parse legendary JSON: {e}");
            return Ok(Vec::new());
        }
    };

    let mut results: Vec<EpicDiscoveredGame> = games
        .into_iter()
        .filter(|g| g.install_path.exists())
        .map(|g| {
            EpicDiscoveredGame::new(
                g.title,
                g.app_name,
                g.install_path,
                g.executable,
                g.launch_parameters,
            )
        })
        .collect();

    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });

    Ok(results)
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
}
