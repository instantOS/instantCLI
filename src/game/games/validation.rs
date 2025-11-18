use crate::game::config::InstantGameConfig;
use anyhow::{Context, Result};


/// Validate game manager is initialized
pub fn validate_game_manager_initialized() -> Result<bool> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    if !config.is_initialized() {
        eprintln!("Game save manager is not initialized!");
        eprintln!("Please run '{} game init' first.", env!("CARGO_BIN_NAME"));
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
