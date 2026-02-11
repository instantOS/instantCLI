use super::add::{AddGameOptions, ResolvedGameDetails};
use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
use anyhow::{Context, Result};

pub(super) struct GameCreationContext {
    config: InstantGameConfig,
    installations: InstallationsConfig,
}

impl GameCreationContext {
    pub(super) fn load() -> Result<Self> {
        Ok(Self {
            config: InstantGameConfig::load().context("Failed to load game configuration")?,
            installations: InstallationsConfig::load()
                .context("Failed to load installations configuration")?,
        })
    }

    pub(super) fn config(&self) -> &InstantGameConfig {
        &self.config
    }

    pub(super) fn config_mut(&mut self) -> &mut InstantGameConfig {
        &mut self.config
    }

    pub(super) fn installations_mut(&mut self) -> &mut InstallationsConfig {
        &mut self.installations
    }

    pub(super) fn installations(&self) -> &InstallationsConfig {
        &self.installations
    }

    pub(super) fn game_exists(&self, name: &str) -> bool {
        self.config.games.iter().any(|g| g.name.0 == name)
    }

    pub(super) fn save(&self) -> Result<()> {
        self.config.save()?;
        self.installations.save()?;
        Ok(())
    }
}

/// Manage game CRUD operations
pub struct GameManager;

impl GameManager {
    /// Add a new game to the configuration
    pub fn add_game(options: AddGameOptions) -> Result<()> {
        let mut context = GameCreationContext::load()?;

        if !super::validation::validate_game_manager_initialized()? {
            return Ok(());
        }

        if options.name.is_none() {
            match super::add::maybe_prefill_from_emulators(options, &context)? {
                super::add::EmulatorPrefillResult::OpenGameMenu(game_name) => {
                    return crate::game::menu::game_menu(Some(game_name));
                }
                super::add::EmulatorPrefillResult::Continue(new_options) => {
                    let details = super::add::resolve_add_game_details(new_options, &context)?;
                    return Self::finish_add_game(&mut context, details);
                }
            }
        }

        let details = super::add::resolve_add_game_details(options, &context)?;
        Self::finish_add_game(&mut context, details)
    }

    fn finish_add_game(
        context: &mut GameCreationContext,
        details: ResolvedGameDetails,
    ) -> Result<()> {
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
        super::remove::remove_game(game_name, force)
    }

    /// Relocate a game's save path to point to a new location (does not move files)
    pub fn relocate_game(game_name: Option<String>, new_path: Option<String>) -> Result<()> {
        let mut context = GameCreationContext::load()?;

        let game_name = match super::relocate::resolve_relocation_game_name(game_name)? {
            Some(name) => name,
            None => return Ok(()),
        };

        if !super::relocate::ensure_game_exists(&context, &game_name)? {
            return Ok(());
        }

        let new_path = super::relocate::resolve_relocation_save_path(&game_name, new_path)?;
        let save_path_type = super::relocate::determine_save_path_type(&new_path)?;

        super::relocate::upsert_installation(
            &mut context.installations_mut().installations,
            &game_name,
            new_path.clone(),
            save_path_type,
        );

        context.save()?;

        let path_display = new_path
            .to_tilde_string()
            .unwrap_or_else(|_| new_path.as_path().to_string_lossy().to_string());

        println!("✓ Save path for '{game_name}' relocated successfully!");
        println!("New save path: {}", path_display);

        Ok(())
    }

    pub(crate) fn get_game_description() -> Result<String> {
        super::prompts::get_game_description()
    }

    pub(crate) fn get_launch_command() -> Result<String> {
        super::prompts::get_launch_command()
    }
}
