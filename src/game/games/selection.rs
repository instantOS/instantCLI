use crate::game::config::{Game, InstallationsConfig, InstantGameConfig, PathContentKind};
use crate::game::utils::save_files::{
    format_file_size, format_system_time_for_display, get_save_directory_info,
};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{FzfSelectable, FzfWrapper};
use anyhow::{Context, Result};
use colored::*;

impl FzfSelectable for Game {
    fn fzf_display_text(&self) -> String {
        self.name.0.clone()
    }

    fn fzf_key(&self) -> String {
        self.name.0.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        // Try to load installation data for more detailed preview
        match (
            InstallationsConfig::load(),
            &self.description,
            &self.launch_command,
        ) {
            (Ok(installations), description, launch_command) => {
                let installation = installations
                    .installations
                    .iter()
                    .find(|inst| inst.game_name.0 == self.name.0);

                let preview_text =
                    self.generate_preview_text(description, launch_command, installation);
                FzfPreview::Text(preview_text)
            }
            (Err(_), description, launch_command) => {
                // Fallback to basic info if installations config can't be loaded
                let mut preview = String::new();

                if let Some(desc) = description {
                    preview.push_str(&format!(" {}\n\n", desc));
                }

                if let Some(command) = launch_command {
                    preview.push_str(&format!(" Launch: {}\n", command));
                }

                if preview.is_empty() {
                    preview.push_str("No additional information available.");
                }

                FzfPreview::Text(preview)
            }
        }
    }
}

impl Game {
    fn generate_preview_text(
        &self,
        description: &Option<String>,
        launch_command: &Option<String>,
        installation: Option<&crate::game::config::GameInstallation>,
    ) -> String {
        let mut text = String::new();

        // Game name
        text.push_str(&format!("{}\n", self.name.0.cyan().bold()));
        text.push('\n');

        // Description
        if let Some(desc) = description {
            text.push_str(&format!(" {}\n\n", desc));
        }

        // Launch command
        if let Some(command) = launch_command {
            text.push_str(&format!(" Launch: {}\n", command.blue()));
        }

        // Installation information
        if let Some(install) = installation {
            text.push_str(&format!(
                "\n{} Installation:\n",
                char::from(crate::ui::nerd_font::NerdFont::Folder)
            ));

            let path_display = install
                .save_path
                .to_tilde_string()
                .unwrap_or_else(|_| install.save_path.as_path().to_string_lossy().to_string());

            text.push_str(&format!(
                "  {} Save Path: {}\n",
                char::from(crate::ui::nerd_font::NerdFont::Folder),
                path_display.green()
            ));

            // Try to get save directory stats
            match get_save_directory_info(install.save_path.as_path()) {
                Ok(info) => {
                    if info.file_count > 0 {
                        text.push_str(&format!(
                            "  {} Local Saves:\n",
                            char::from(crate::ui::nerd_font::NerdFont::Save)
                        ));
                        text.push_str(&format!(
                            "     • Last modified: {}\n",
                            format_system_time_for_display(info.last_modified)
                        ));
                        text.push_str(&format!("     • Files: {}\n", info.file_count));
                        text.push_str(&format!(
                            "     • Total size: {}\n",
                            format_file_size(info.total_size)
                        ));
                    } else {
                        text.push_str(&format!(
                            "  {} Local Saves: No save files found\n",
                            char::from(crate::ui::nerd_font::NerdFont::Save)
                        ));
                    }
                }
                Err(_) => {
                    text.push_str(&format!(
                        "  {} Local Saves: Unable to analyze\n",
                        char::from(crate::ui::nerd_font::NerdFont::Save)
                    ));
                }
            }

            // Dependencies count
            if !install.dependencies.is_empty() {
                text.push_str(&format!(
                    "  {} Dependencies: {}\n",
                    char::from(crate::ui::nerd_font::NerdFont::Package),
                    install.dependencies.len()
                ));
            }

            // Checkpoint info
            if let Some(checkpoint) = &install.nearest_checkpoint {
                text.push_str(&format!(
                    "  {} Checkpoint: {}\n",
                    char::from(crate::ui::nerd_font::NerdFont::Flag),
                    checkpoint
                ));
            }
        } else {
            text.push_str(&format!(
                "\n{} No installation data found\n",
                char::from(crate::ui::nerd_font::NerdFont::Warning)
            ));
        }

        // Game dependencies (from game config)
        if !self.dependencies.is_empty() {
            text.push_str(&format!(
                "\n{} Configured Dependencies: {}\n",
                char::from(crate::ui::nerd_font::NerdFont::Package),
                self.dependencies.len()
            ));
            for dep in self.dependencies.iter().take(3) {
                // Show first 3 dependencies
                text.push_str(&format!(
                    "  • {} ({})\n",
                    dep.id,
                    kind_label(dep.source_type)
                ));
            }
            if self.dependencies.len() > 3 {
                text.push_str(&format!("  ... and {} more\n", self.dependencies.len() - 3));
            }
        }

        text
    }
}

/// Helper function to get human-readable label for PathContentKind
fn kind_label(kind: PathContentKind) -> &'static str {
    match kind {
        PathContentKind::Directory => "Directory",
        PathContentKind::File => "File",
    }
}

/// Helper function to select a game interactively
/// Returns Some(game_name) if a game was selected, None if cancelled
pub fn select_game_interactive(prompt_message: Option<&str>) -> Result<Option<String>> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    if config.games.is_empty() {
        println!("No games configured yet.");
        println!("Use '{} game add' to add a game.", env!("CARGO_BIN_NAME"));
        return Ok(None);
    }

    // Show FZF menu to select game
    if let Some(message) = prompt_message {
        FzfWrapper::message(message).context("Failed to show selection prompt")?;
    }
    let selected = FzfWrapper::select_one(config.games.clone())
        .map_err(|e| anyhow::anyhow!("Failed to select game: {}", e))?;

    match selected {
        Some(game) => Ok(Some(game.name.0)),
        None => {
            println!("No game selected.");
            Ok(None)
        }
    }
}
