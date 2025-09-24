use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::InstantGameConfig;
use anyhow::{Context, Result};

/// Validate game name uniqueness
pub fn validate_game_name_unique(name: &str) -> Result<bool> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    Ok(!config.games.iter().any(|g| g.name.0 == name))
}

/// Check if game exists and return error message if not
pub fn validate_game_exists(name: &str) -> Result<bool> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    let exists = config.games.iter().any(|g| g.name.0 == name);

    if !exists {
        FzfWrapper::message(&format!("Game '{name}' not found in configuration."))
            .context("Failed to show game not found message")?;
    }

    Ok(exists)
}

/// Validate game manager is initialized
pub fn validate_game_manager_initialized() -> Result<bool> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    if !config.is_initialized() {
        FzfWrapper::message(
            "Game save manager is not initialized!\n\nPlease run 'instant game init' first.",
        )
        .context("Failed to show initialization required message")?;
        Ok(false)
    } else {
        Ok(true)
    }
}

/// Validate non-empty input with custom error message
pub fn validate_non_empty(input: &str, field_name: &str) -> Result<bool> {
    if input.is_empty() {
        FzfWrapper::message(&format!("{field_name} cannot be empty."))
            .context("Failed to show validation error")?;
        Ok(false)
    } else {
        Ok(true)
    }
}
