use crate::game::config::{Game, InstallationsConfig, InstantGameConfig, PathContentKind};
use crate::game::utils::save_files::{
    format_file_size, format_system_time_for_display, get_save_directory_info,
};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, MenuCursor};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

/// Menu entry for game selection - can be a game or a special action
#[derive(Debug, Clone)]
pub enum GameMenuEntry {
    Game(String),
    AddGame,
    SetupGames,
    CloseMenu,
}

impl FzfSelectable for GameMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            GameMenuEntry::Game(name) => name.clone(),
            GameMenuEntry::AddGame => {
                format!("{} Add Game", char::from(NerdFont::Plus))
            }
            GameMenuEntry::SetupGames => {
                format!("{} Set Up Existing Games", char::from(NerdFont::Wrench))
            }
            GameMenuEntry::CloseMenu => {
                format!("{} Close Menu", char::from(NerdFont::Cross))
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            GameMenuEntry::Game(name) => name.clone(),
            GameMenuEntry::AddGame => "!__add_game__".to_string(),
            GameMenuEntry::SetupGames => "!__setup_games__".to_string(),
            GameMenuEntry::CloseMenu => "!__close_menu__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        use crate::ui::preview::PreviewBuilder;

        match self {
            GameMenuEntry::Game(name) => {
                // Try to load game for preview
                match (InstantGameConfig::load(), InstallationsConfig::load()) {
                    (Ok(game_config), Ok(installations)) => {
                        let game = game_config.games.iter().find(|g| g.name.0 == *name);
                        let installation = installations
                            .installations
                            .iter()
                            .find(|inst| inst.game_name.0 == *name);

                        if let Some(game) = game {
                            FzfPreview::Text(game.generate_preview_text(
                                &game.description,
                                &game.launch_command,
                                installation,
                            ))
                        } else {
                            FzfPreview::Text(format!("Game: {}", name))
                        }
                    }
                    _ => FzfPreview::Text(format!("Game: {}", name)),
                }
            }
            GameMenuEntry::AddGame => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Game")
                .text("Add a new game to track.")
                .blank()
                .text("This will guide you through:")
                .bullet("Setting a game name and description")
                .bullet("Configuring the launch command")
                .bullet("Selecting the save data location")
                .build(),
            GameMenuEntry::SetupGames => PreviewBuilder::new()
                .header(NerdFont::Wrench, "Set Up Existing Games")
                .text("Configure games that need setup.")
                .blank()
                .text("This helps with:")
                .bullet("Games registered but missing save paths")
                .bullet("Games with pending dependencies")
                .bullet("Restoring games from backups")
                .build(),
            GameMenuEntry::CloseMenu => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close Menu")
                .text("Close the game menu.")
                .blank()
                .text("This will exit the interactive game menu")
                .text("and return to the command prompt.")
                .build(),
        }
    }
}

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
        use crate::ui::catppuccin::colors;
        use crate::ui::preview::PreviewBuilder;

        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Gamepad, &self.name.0)
            .blank();

        // Description
        if let Some(desc) = description {
            builder = builder.text(desc).blank();
        }

        // Launch command
        if let Some(command) = launch_command {
            builder = builder.field("Launch", command);
        }

        // Installation section
        if let Some(install) = installation {
            let path_display = install
                .save_path
                .to_tilde_string()
                .unwrap_or_else(|_| install.save_path.as_path().to_string_lossy().to_string());

            builder = builder
                .blank()
                .separator()
                .blank()
                .line(colors::GREEN, Some(NerdFont::Desktop), "This Device")
                .blank()
                .field_indented("Save Path", &path_display);

            // Save directory stats
            match get_save_directory_info(install.save_path.as_path()) {
                Ok(info) if info.file_count > 0 => {
                    builder = builder
                        .field_indented(
                            "Last Modified",
                            &format_system_time_for_display(info.last_modified),
                        )
                        .field_indented("Files", &info.file_count.to_string())
                        .field_indented("Size", &format_file_size(info.total_size));
                }
                Ok(_) => {
                    builder = builder.field_indented("Local Saves", "No save files found");
                }
                Err(_) => {
                    builder = builder.field_indented("Local Saves", "Unable to analyze");
                }
            }

            // Checkpoint
            if let Some(checkpoint) = &install.nearest_checkpoint {
                builder = builder.field_indented("Checkpoint", checkpoint);
            }

            // Dependencies
            if !install.dependencies.is_empty() {
                builder =
                    builder.field_indented("Dependencies", &install.dependencies.len().to_string());
            }
        } else {
            builder = builder.blank().separator().blank().line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "Not set up on this device",
            );
        }

        // Game dependencies (from config)
        if !self.dependencies.is_empty() {
            builder = builder.blank().separator().blank().line(
                colors::MAUVE,
                Some(NerdFont::Package),
                &format!("Dependencies ({})", self.dependencies.len()),
            );

            for dep in self.dependencies.iter().take(3) {
                builder = builder.bullet(&format!("{} ({})", dep.id, kind_label(dep.source_type)));
            }

            if self.dependencies.len() > 3 {
                builder = builder.subtext(&format!("  … and {} more", self.dependencies.len() - 3));
            }
        }

        builder.build_string()
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

    let result = FzfWrapper::builder()
        .args(crate::ui::catppuccin::fzf_mocha_args())
        .responsive_layout()
        .select(config.games.clone())
        .map_err(|e| anyhow::anyhow!("Failed to select game: {}", e))?;

    let selected = match result {
        FzfResult::Selected(game) => Some(game),
        _ => None,
    };

    match selected {
        Some(game) => Ok(Some(game.name.0)),
        None => {
            println!("No game selected.");
            Ok(None)
        }
    }
}

/// Select a game menu entry (game or special action)
/// Returns Some(entry) if selected, None if cancelled
pub fn select_game_menu_entry(cursor: &mut MenuCursor) -> Result<Option<GameMenuEntry>> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    // Build menu entries: special actions first, then games
    let mut entries = vec![
        GameMenuEntry::AddGame,
        GameMenuEntry::SetupGames,
        GameMenuEntry::CloseMenu,
    ];

    // Add all games
    for game in &config.games {
        entries.push(GameMenuEntry::Game(game.name.0.clone()));
    }

    let mut builder = FzfWrapper::builder()
        .header("Game Menu")
        .prompt("Select")
        .args(crate::ui::catppuccin::fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(index);
    }

    // Show menu
    let result = builder
        .select(entries.clone())
        .map_err(|e| anyhow::anyhow!("Failed to select from game menu: {}", e))?;

    match result {
        FzfResult::Selected(entry) => {
            cursor.update(&entry, &entries);
            Ok(Some(entry))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}
