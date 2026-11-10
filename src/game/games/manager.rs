use super::add::{AddGameOptions, ResolvedGameDetails};
use super::add_discovery::{EmulatorPrefillResult, maybe_prefill_from_emulators};
use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

pub(super) struct GameCreationContext {
    pub(super) config: InstantGameConfig,
    pub(super) installations: InstallationsConfig,
}

impl GameCreationContext {
    pub(super) fn load() -> Result<Self> {
        Ok(Self {
            config: InstantGameConfig::load().context("Failed to load game configuration")?,
            installations: InstallationsConfig::load()
                .context("Failed to load installations configuration")?,
        })
    }

    pub(super) fn game_exists(&self, name: &str) -> bool {
        self.config.games.iter().any(|g| g.name.0 == name)
    }

    pub(super) fn installation_exists(&self, name: &str) -> bool {
        self.config
            .resolve_owner(&self.installations, &self.config.owner_id(name))
            .is_ok()
    }

    pub(super) fn save(&self) -> Result<()> {
        crate::game::config::save_pair(&self.config, &self.installations)?;
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
            loop {
                match maybe_prefill_from_emulators(options.clone(), &context)? {
                    EmulatorPrefillResult::OpenGameMenu(game_name) => {
                        return crate::game::menu::game_menu(Some(game_name));
                    }
                    EmulatorPrefillResult::SetupTrackedGame {
                        game_name,
                        discovered_save_path,
                    } => {
                        return crate::game::setup::setup_tracked_game(
                            &game_name,
                            discovered_save_path.as_deref(),
                        );
                    }
                    EmulatorPrefillResult::OpenPrefilledSharedAddEditor { options, group } => {
                        return crate::game::menu::open_prefilled_shared_add_editor(
                            options, &group,
                        );
                    }
                    EmulatorPrefillResult::OpenPrefilledAddEditor(new_options) => {
                        return crate::game::menu::open_prefilled_add_editor(new_options);
                    }
                    EmulatorPrefillResult::Continue(new_options) => {
                        let Some(details) =
                            super::add::resolve_add_game_details(new_options, &context)?
                        else {
                            continue;
                        };
                        return Self::finish_add_game(&mut context, details);
                    }
                    EmulatorPrefillResult::Cancelled => {
                        return Ok(());
                    }
                }
            }
        }

        let Some(details) = super::add::resolve_add_game_details(options, &context)? else {
            return Ok(());
        };
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
        game.launch_command = details.launch_command;

        context.config.games.push(game);

        let save_path_display = details
            .saves
            .as_ref()
            .map(|saves| saves.save_path.display_string());
        if let Some(saves) = details.saves {
            let installation = GameInstallation::with_kind(
                details.name.clone(),
                saves.save_path,
                saves.save_path_type,
            );
            context.installations.installations.push(installation);
        }

        context.save()?;

        println!(
            "{} Game '{}' added successfully!",
            NerdFont::Check,
            details.name
        );
        if let Some(path) = save_path_display {
            println!("Game configuration saved with save path: {path}");
        } else {
            crate::ui::emit(
                crate::ui::Level::Warn,
                "game.add.backups_not_configured",
                &super::prompts::unconfigured_saves_warning(&details.name),
                Some(serde_json::json!({"game": details.name, "backups_configured": false})),
            );
        }

        Ok(())
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

        let id = context.config.owner_id(&game_name);
        let _lock = crate::game::save_runtime::SaveLock::owner(&id)?;
        anyhow::ensure!(
            matches!(id, crate::game::ownership::OwnerId::Game(_)),
            "This game uses shared saves; use ins game group bind to relocate the whole group"
        );
        let new_path = super::relocate::resolve_relocation_save_path(&game_name, new_path)?;
        let save_path_type = super::relocate::determine_save_path_type(&new_path)?;

        super::relocate::upsert_installation(
            &mut context.installations.installations,
            &game_name,
            new_path.clone(),
            save_path_type,
        );

        context.save()?;

        let path_display = new_path.display_string();

        println!(
            "{} Save path for '{game_name}' relocated successfully!",
            NerdFont::Check
        );
        println!("New save path: {}", path_display);

        Ok(())
    }
}
