use super::{selection::select_game_interactive, validation::*};
use crate::common::TildePath;
use crate::game::config::{
    Game, GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind,
};
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result, anyhow};
use std::fs;

/// Options for adding a game non-interactively
#[derive(Debug, Default)]
pub struct AddGameOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub launch_command: Option<String>,
    pub save_path: Option<String>,
    pub create_save_path: bool,
}

struct GameCreationContext {
    config: InstantGameConfig,
    installations: InstallationsConfig,
}

impl GameCreationContext {
    fn load() -> Result<Self> {
        Ok(Self {
            config: InstantGameConfig::load().context("Failed to load game configuration")?,
            installations: InstallationsConfig::load()
                .context("Failed to load installations configuration")?,
        })
    }

    fn config(&self) -> &InstantGameConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut InstantGameConfig {
        &mut self.config
    }

    fn installations_mut(&mut self) -> &mut InstallationsConfig {
        &mut self.installations
    }

    fn game_exists(&self, name: &str) -> bool {
        self.config.games.iter().any(|g| g.name.0 == name)
    }

    fn save(&self) -> Result<()> {
        self.config.save()?;
        self.installations.save()?;
        Ok(())
    }
}

struct ResolvedGameDetails {
    name: String,
    description: Option<String>,
    launch_command: Option<String>,
    save_path: TildePath,
    save_path_type: PathContentKind,
}

/// Manage game CRUD operations
pub struct GameManager;

impl GameManager {
    /// Add a new game to the configuration
    pub fn add_game(options: AddGameOptions) -> Result<()> {
        let mut context = GameCreationContext::load()?;

        if !validate_game_manager_initialized()? {
            return Ok(());
        }

        let details = resolve_add_game_details(options, &context)?;

        let mut game = Game::new(details.name.clone());
        if let Some(description) = &details.description {
            game.description = Some(description.clone());
        }
        if let Some(command) = &details.launch_command {
            game.launch_command = Some(command.clone());
        }

        context.config_mut().games.push(game);

        context
            .installations_mut()
            .installations
            .push(GameInstallation::with_kind(
                details.name.clone(),
                details.save_path.clone(),
                details.save_path_type,
            ));

        context.save()?;

        let save_path_display = details
            .save_path
            .to_tilde_string()
            .unwrap_or_else(|_| details.save_path.as_path().to_string_lossy().to_string());

        println!("✓ Game '{}' added successfully!", details.name);
        println!(
            "Game configuration saved with save path: {}",
            save_path_display
        );

        Ok(())
    }

    /// Remove a game from the configuration
    pub fn remove_game(game_name: Option<String>, force: bool) -> Result<()> {
        let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;

        let mut installations =
            InstallationsConfig::load().context("Failed to load installations configuration")?;

        let game_name = match game_name {
            Some(name) => name,
            None => match select_game_interactive(Some("Select a game to remove:"))? {
                Some(name) => name,
                None => return Ok(()),
            },
        };

        // Find the game in the configuration
        let game_index = config.games.iter().position(|g| g.name.0 == game_name);

        if game_index.is_none() {
            eprintln!("Game '{game_name}' not found in configuration.");
            return Ok(());
        }

        let game_index = game_index.unwrap();

        if force {
            config.games.remove(game_index);
            config.save()?;

            installations
                .installations
                .retain(|inst| inst.game_name.0 != game_name);
            installations.save()?;

            println!("✓ Game '{game_name}' removed successfully!");
            return Ok(());
        }

        let game = &config.games[game_index];

        // Show game details and ask for confirmation with improved formatting
        match FzfWrapper::builder()
            .confirm(format!(
                "Are you sure you want to remove the following game?\n\n\
                 Game: {}\n\
                 Description: {}\n\
                 Launch command: {}\n\n\
                 This will remove the game from your configuration and save path mapping.",
                game.name.0,
                game.description.as_deref().unwrap_or("None"),
                game.launch_command.as_deref().unwrap_or("None")
            ))
            .yes_text("Remove Game")
            .no_text("Keep Game")
            .show_confirmation()
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
        {
            ConfirmResult::Yes => {
                // Remove the game from the configuration
                config.games.remove(game_index);
                config.save()?;

                // Remove any installations for this game
                installations
                    .installations
                    .retain(|inst| inst.game_name.0 != game_name);
                installations.save()?;

                println!("✓ Game '{game_name}' removed successfully!");
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                println!("Game removal cancelled.");
            }
        }

        Ok(())
    }

    /// Get and validate game name from user input
    fn get_game_name(config: &InstantGameConfig) -> Result<String> {
        let game_name = FzfWrapper::input("Enter game name")
            .map_err(|e| anyhow::anyhow!("Failed to get game name input: {}", e))?
            .trim()
            .to_string();

        if !validate_non_empty(&game_name, "Game name")? {
            return Err(anyhow::anyhow!("Game name cannot be empty"));
        }

        // Check if game already exists
        if config.games.iter().any(|g| g.name.0 == game_name) {
            eprintln!("Game '{game_name}' already exists!");
            return Err(anyhow::anyhow!("Game already exists"));
        }

        Ok(game_name)
    }

    /// Get optional game description from user input
    pub(crate) fn get_game_description() -> Result<String> {
        Ok(FzfWrapper::input("Enter game description (optional)")
            .map_err(|e| anyhow::anyhow!("Failed to get description input: {}", e))?
            .trim()
            .to_string())
    }

    /// Get optional launch command from user input
    pub(crate) fn get_launch_command() -> Result<String> {
        Ok(FzfWrapper::input("Enter launch command (optional)")
            .map_err(|e| anyhow::anyhow!("Failed to get launch command input: {}", e))?
            .trim()
            .to_string())
    }

    /// Get save path from user input with validation
    fn get_save_path(game_name: &str) -> Result<TildePath> {
        let selection = PathInputBuilder::new()
            .header(format!(
                "{} Choose the save path for '{game_name}'",
                char::from(NerdFont::Folder)
            ))
            .manual_prompt(format!(
                "{} Enter the save path (e.g., ~/.local/share/{}/saves)",
                char::from(NerdFont::Edit),
                game_name.to_lowercase().replace(' ', "-")
            ))
            .scope(FilePickerScope::FilesAndDirectories)
            .picker_hint(format!(
                "{} Select the file or directory that stores the save data",
                char::from(NerdFont::Info)
            ))
            .manual_option_label(format!("{} Type an exact path", char::from(NerdFont::Edit)))
            .picker_option_label(format!(
                "{} Browse and choose a path",
                char::from(NerdFont::FolderOpen)
            ))
            .choose()?;

        let save_path = match selection {
            PathInputSelection::Manual(input) => {
                if !validate_non_empty(&input, "Save path")? {
                    return Err(anyhow::anyhow!("Save path cannot be empty"));
                }
                TildePath::from_str(&input)
                    .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?
            }
            PathInputSelection::Picker(path) => TildePath::new(path),
            PathInputSelection::WinePrefix(path) => TildePath::new(path),
            PathInputSelection::Cancelled => {
                println!("Game addition cancelled: save path not provided.");
                return Err(anyhow::anyhow!("Save path selection cancelled"));
            }
        };

        if let Err(err) = ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory) {
            println!("{} {}", char::from(NerdFont::CrossCircle), err);
            return Self::get_save_path(game_name);
        }

        let save_path_display = save_path
            .to_tilde_string()
            .unwrap_or_else(|_| save_path.as_path().to_string_lossy().to_string());

        // Confirm the selected save path with the user
        match FzfWrapper::builder()
            .confirm(format!(
                "{} Are you sure you want to use '{save_path_display}' as the save path for '{game_name}'?\n\n\
                This path will be used to store and sync save files for this game.",
                char::from(NerdFont::Question)
            ))
            .yes_text("Use This Path")
            .no_text("Choose Different Path")
            .confirm_dialog()
            .map_err(|e| anyhow::anyhow!("Failed to get path confirmation: {}", e))?
        {
            ConfirmResult::Yes => {
                // Continue with the selected path
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                println!("{} Choosing different save path...", char::from(NerdFont::Info));
                return Self::get_save_path(game_name);
            }
        }

        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "{} Save path '{save_path_display}' does not exist. Create it?",
                char::from(NerdFont::Warning)
            ))
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => {
                    std::fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    println!(
                        "{} Created save directory: {save_path_display}",
                        char::from(NerdFont::Check)
                    );
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!(
                        "{} Game addition cancelled: save path does not exist.",
                        char::from(NerdFont::Warning)
                    );
                    return Err(anyhow::anyhow!("Save path does not exist"));
                }
            }
        }

        Ok(save_path)
    }
}

fn resolve_add_game_details(
    options: AddGameOptions,
    context: &GameCreationContext,
) -> Result<ResolvedGameDetails> {
    let AddGameOptions {
        name,
        description,
        launch_command,
        save_path,
        create_save_path,
    } = options;

    let interactive_prompts = name.is_none();

    let game_name = match name {
        Some(raw_name) => {
            let trimmed = raw_name.trim();
            if !validate_non_empty(trimmed, "Game name")? {
                return Err(anyhow!("Game name cannot be empty"));
            }

            if context.game_exists(trimmed) {
                return Err(anyhow!("Game '{}' already exists", trimmed));
            }

            trimmed.to_string()
        }
        None => GameManager::get_game_name(context.config())?,
    };

    let description = match description {
        Some(text) => some_if_not_empty(text),
        None if interactive_prompts => some_if_not_empty(GameManager::get_game_description()?),
        None => None,
    };

    let launch_command = match launch_command {
        Some(command) => some_if_not_empty(command),
        None if interactive_prompts => some_if_not_empty(GameManager::get_launch_command()?),
        None => None,
    };

    let save_path = match save_path {
        Some(path) => {
            let trimmed = path.trim();
            if !validate_non_empty(trimmed, "Save path")? {
                return Err(anyhow!("Save path cannot be empty"));
            }

            let tilde_path =
                TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid save path: {}", e))?;

            ensure_safe_path(tilde_path.as_path(), PathUsage::SaveDirectory)?;

            if !tilde_path.as_path().exists() {
                if create_save_path {
                    fs::create_dir_all(tilde_path.as_path())
                        .context("Failed to create save directory")?;
                    println!(
                        "{} Created save directory: {}",
                        char::from(NerdFont::Check),
                        trimmed
                    );
                } else {
                    return Err(anyhow!(
                        "Save path '{}' does not exist. Use --create-save-path to create it automatically or run '{} game add' without --save-path for interactive setup.",
                        tilde_path.as_path().display(),
                        env!("CARGO_BIN_NAME")
                    ));
                }
            }

            tilde_path
        }
        None => GameManager::get_save_path(&game_name)?,
    };

    ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory)?;

    let save_path_type = if save_path.as_path().exists() {
        let metadata = fs::metadata(save_path.as_path()).with_context(|| {
            format!(
                "Failed to read metadata for save path: {}",
                save_path.as_path().display()
            )
        })?;
        PathContentKind::from(metadata)
    } else {
        PathContentKind::Directory
    };

    Ok(ResolvedGameDetails {
        name: game_name,
        description,
        launch_command,
        save_path,
        save_path_type,
    })
}

fn some_if_not_empty(value: impl Into<String>) -> Option<String> {
    let text = value.into();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
