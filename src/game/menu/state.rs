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
        self.game_config
            .save()
            .context("Failed to save games.toml")?;
        self.installations
            .save()
            .context("Failed to save installations.toml")?;
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
