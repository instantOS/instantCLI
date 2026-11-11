use anyhow::{Context, Result};

use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::ui::nerd_font::NerdFont;

/// Manages the state of the game edit session
pub struct EditState {
    pub game_config: InstantGameConfig,
    pub installations: InstallationsConfig,
    pub game_index: usize,
    pub installation_index: Option<usize>,
    pub dirty: bool,
    pub discovery_launch_hint: Option<crate::game::platforms::discovery::DiscoveryLaunchHint>,
}

impl EditState {
    /// Create a new edit state with loaded configurations
    pub fn new(
        game_config: InstantGameConfig,
        installations: InstallationsConfig,
        game_index: usize,
        installation_index: Option<usize>,
    ) -> Self {
        Self {
            game_config,
            installations,
            game_index,
            installation_index,
            dirty: false,
            discovery_launch_hint: None,
        }
    }

    /// Mark the state as dirty (has unsaved changes)
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Check if there are unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Save both configuration files
    pub fn save(&mut self) -> Result<()> {
        let _lock = crate::game::save_runtime::SaveLock::owner(
            &self.game_config.owner_id(&self.game().name.0),
        )?;
        crate::game::config::save_pair(&self.game_config, &self.installations)
            .context("Failed to save game configuration")?;
        self.dirty = false;
        println!(
            "{} Changes saved successfully.",
            char::from(NerdFont::Check)
        );
        Ok(())
    }

    /// Get a reference to the current game
    pub fn game(&self) -> &crate::game::config::Game {
        &self.game_config.games[self.game_index]
    }

    /// Get a mutable reference to the current game
    pub fn game_mut(&mut self) -> &mut crate::game::config::Game {
        &mut self.game_config.games[self.game_index]
    }

    /// Get a reference to the current installation (if exists)
    pub fn installation(&self) -> Option<&crate::game::config::GameInstallation> {
        self.installation_index
            .map(|idx| &self.installations.installations[idx])
    }

    /// Get a mutable reference to the current installation (if exists)
    pub fn installation_mut(&mut self) -> Option<&mut crate::game::config::GameInstallation> {
        self.installation_index
            .map(|idx| &mut self.installations.installations[idx])
    }
}

/// Loaded game state to avoid repeated config loading
pub(super) struct GameState {
    pub(super) game_config: InstantGameConfig,
    pub(super) installations: InstallationsConfig,
    pub(super) needs_setup: bool,
    pub(super) launch_command: Option<String>,
    pub(super) description: Option<String>,
    pub(super) save_path: Option<String>,
}

impl GameState {
    pub(super) fn load(game_name: &str) -> Result<Self> {
        let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
        let installations =
            InstallationsConfig::load().context("Failed to load installations configuration")?;

        let installation = installations
            .installations
            .iter()
            .find(|i| i.game_name.0 == game_name);

        let game = game_config.games.iter().find(|g| g.name.0 == game_name);
        let game_cmd = game.and_then(|g| g.launch_command.clone());
        let description = game.and_then(|g| g.description.clone());

        let inst_cmd = installation.and_then(|i| i.launch_command.clone());

        let resolved = game_config
            .resolve_owner(&installations, &game_config.owner_id(game_name))
            .ok();
        let save_path = resolved.as_ref().map(|i| {
            i.save_path
                .to_tilde_string()
                .unwrap_or_else(|_| i.save_path.as_path().to_string_lossy().to_string())
        });

        // Installation command takes precedence over game command
        let launch_command = inst_cmd.or(game_cmd).map(|command| command.to_string());

        let needs_setup = resolved.is_none();
        Ok(Self {
            game_config,
            installations,
            needs_setup,
            launch_command,
            description,
            save_path,
        })
    }
}
