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
        eprintln!("Game '{name}' not found in configuration.");
    }

    Ok(exists)
}

/// Validate game manager is initialized
pub fn validate_game_manager_initialized() -> Result<bool> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    if !config.is_initialized() {
        eprintln!("Game save manager is not initialized!");
        eprintln!("Please run 'instant game init' first.");
        Ok(false)
    } else {
        Ok(true)
    }
}

/// Validate non-empty input with custom error message
pub fn validate_non_empty(input: &str, field_name: &str) -> Result<bool> {
    if input.is_empty() {
        eprintln!("{field_name} cannot be empty.");
        Ok(false)
    } else {
        Ok(true)
    }
}
