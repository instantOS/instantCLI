use crate::game::config::{InstallationsConfig, InstantGameConfig, PathContentKind};
use crate::game::utils::save_files::{
    format_file_size, format_system_time_for_display, get_save_directory_info,
};
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::*;
use serde_json::json;

/// Display list of all configured games
pub fn list_games() -> Result<()> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    // Prepare data array
    let games_data: Vec<serde_json::Value> = config
        .games
        .iter()
        .map(|g| {
            json!({
                "name": g.name.0,
                "description": g.description,
                "launch_command": g.launch_command
            })
        })
        .collect();

    // Build human-friendly text block
    let mut text = String::new();
    if config.games.is_empty() {
        text.push_str("No games configured yet.\n");
        text.push_str(&format!(
            "Use '{} game add' to add a game.\n",
            env!("CARGO_BIN_NAME")
        ));
    } else {
        text.push_str(&format!("{}\n\n", "Configured Games".bold().underline()));
        for game in &config.games {
            text.push_str(&format!(
                "  {} {}\n",
                char::from(NerdFont::Info).to_string().bright_blue(),
                game.name.0.cyan().bold()
            ));
            if let Some(desc) = &game.description {
                text.push_str(&format!("    Description: {}\n", desc));
            }
            if let Some(cmd) = &game.launch_command {
                text.push_str(&format!("    Launch command: {}\n", cmd.blue()));
            }
            text.push('\n');
        }
        text.push_str(&format!(
            "Total: {} game{} configured",
            config.games.len().to_string().bold(),
            if config.games.len() == 1 { "" } else { "s" }
        ));
    }

    emit(
        Level::Info,
        "game.list",
        &text,
        Some(json!({
            "count": config.games.len(),
            "games": games_data
        })),
    );

    Ok(())
}

/// Display detailed information about a specific game
pub fn show_game_details(game_name: &str) -> Result<()> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    match collect_game_details(&config, &installations, game_name) {
        Some(details) => {
            let text = details.render_text();
            let payload = details.to_json();

            emit(Level::Info, "game.show.details", &text, Some(payload));
        }
        None => {
            emit(
                Level::Error,
                "game.show.not_found",
                &format!(
                    "{} Game '{}' not found in configuration.",
                    char::from(NerdFont::CrossCircle),
                    game_name.red()
                ),
                None,
            );
        }
    }

    Ok(())
}

fn collect_game_details(
    config: &InstantGameConfig,
    installations: &InstallationsConfig,
    game_name: &str,
) -> Option<GameDetails> {
    let game = config.games.iter().find(|g| g.name.0 == game_name)?;
    let installation = installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name);

    let installation_details = installation.map(|install| {
        let path_display = install
            .save_path
            .to_tilde_string()
            .unwrap_or_else(|_| install.save_path.as_path().to_string_lossy().to_string());

        match get_save_directory_info(install.save_path.as_path()) {
            Ok(info) => InstallationDetails {
                path_display,
                stats: Some(SaveDirectoryStats {
                    last_modified: format_system_time_for_display(info.last_modified),
                    file_count: info.file_count,
                    total_size: format_file_size(info.total_size),
                }),
                error: None,
            },
            Err(err) => InstallationDetails {
                path_display,
                stats: None,
                error: Some(err.to_string()),
            },
        }
    });

    let dependencies = game
        .dependencies
        .iter()
        .map(|dependency| {
            let installed_path = installation.and_then(|inst| {
                inst.dependencies
                    .iter()
                    .find(|dep| dep.dependency_id == dependency.id)
                    .and_then(|dep| dep.install_path.to_tilde_string().ok())
            });

            DependencyDetails {
                id: dependency.id.clone(),
                kind: dependency.source_type,
                installed_path,
            }
        })
        .collect();

    Some(GameDetails {
        name: game.name.0.clone(),
        description: game.description.clone(),
        launch_command: game.launch_command.clone(),
        installation: installation_details,
        dependencies,
    })
}

struct GameDetails {
    name: String,
    description: Option<String>,
    launch_command: Option<String>,
    installation: Option<InstallationDetails>,
    dependencies: Vec<DependencyDetails>,
}

struct InstallationDetails {
    path_display: String,
    stats: Option<SaveDirectoryStats>,
    error: Option<String>,
}

struct SaveDirectoryStats {
    last_modified: String,
    file_count: u64,
    total_size: String,
}

struct DependencyDetails {
    id: String,
    kind: PathContentKind,
    installed_path: Option<String>,
}

impl GameDetails {
    fn render_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("{}\n\n", "Game Information".bold().underline()));
        text.push_str(&format!(
            "{} {}\n\n",
            char::from(NerdFont::Info),
            self.name.cyan().bold()
        ));

        if let Some(description) = &self.description {
            text.push_str(&format!(" {}\n\n", description));
        }

        text.push_str(&format!("{}\n", "Configuration:".bold()));
        if let Some(command) = &self.launch_command {
            text.push_str(&format!("   Launch Command: {}\n\n", command.blue()));
        } else {
            text.push('\n');
        }

        if let Some(installation) = &self.installation {
            text.push_str(&installation.render_text());
        } else {
            text.push_str(&format!(
                "{}  No installation data found for this game.\n",
                char::from(NerdFont::Warning)
            ));
        }

        if !self.dependencies.is_empty() {
            text.push('\n');
            text.push_str(&format!(
                "{} Dependencies:\n",
                char::from(NerdFont::Package)
            ));

            for dependency in &self.dependencies {
                text.push_str(&format!(
                    "  • {} ({}) — {}\n",
                    dependency.id,
                    dependency.kind_label(),
                    dependency.status_text()
                ));
            }
        }

        text
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "game": {
                "name": self.name,
                "description": self.description,
                "launch_command": self.launch_command,
            },
            "installation": self.installation.as_ref().map(InstallationDetails::to_json)
        })
    }
}

impl InstallationDetails {
    fn render_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!(
            "Installation:\n  {} Save Path: {}\n",
            char::from(NerdFont::Folder),
            self.path_display.green()
        ));

        match (&self.stats, &self.error) {
            (Some(stats), _) => {
                if stats.file_count > 0 {
                    text.push_str("   Local Saves:\n");
                    text.push_str(&format!(
                        "     • Last modified: {}\n",
                        stats.last_modified
                    ));
                    text.push_str(&format!("     • Files: {}\n", stats.file_count));
                    text.push_str(&format!("     • Total size: {}\n", stats.total_size));
                } else {
                    text.push_str("   Local Saves: No save files found\n");
                }
            }
            (None, Some(error)) => {
                text.push_str(&format!(
                    "   Local Saves: Unable to analyze save directory ({})\n",
                    error.to_lowercase()
                ));
            }
            (None, None) => {
                text.push_str("   Local Saves: Not available\n");
            }
        }

        text
    }

    fn to_json(&self) -> serde_json::Value {
        match (&self.stats, &self.error) {
            (Some(stats), _) => json!({
                "save_path": self.path_display,
                "local_saves": {
                    "last_modified": stats.last_modified,
                    "file_count": stats.file_count,
                    "total_size": stats.total_size
                }
            }),
            (None, Some(error)) => json!({
                "save_path": self.path_display,
                "local_saves_error": error
            }),
            (None, None) => json!({
                "save_path": self.path_display
            }),
        }
    }
}

impl DependencyDetails {
    fn kind_label(&self) -> &'static str {
        match self.kind {
            PathContentKind::Directory => "Directory",
            PathContentKind::File => "File",
        }
    }

    fn status_text(&self) -> String {
        self
            .installed_path
            .as_ref()
            .map(|path| format!("Installed at {path}"))
            .unwrap_or_else(|| "Not installed".to_string())
    }
}
