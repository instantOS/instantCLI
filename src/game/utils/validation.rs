use anyhow::{Context, Result};
use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::InstantGameConfig;
use crate::game::restic::backup;

/// Common validation utilities for game manager operations

/// Check if restic is available and show error message if not
pub fn check_restic_availability() -> Result<()> {
    if !backup::GameBackup::check_restic_availability()? {
        //TODO: this should be a print
        FzfWrapper::message("❌ Error: restic is not installed or not found in PATH.\n\nPlease install restic to use backup functionality.")
            .context("Failed to show restic not available message")?;
        return Err(anyhow::anyhow!("restic not available"));
    }
    Ok(())
}

/// Check if game manager is initialized and show error message if not
pub fn check_game_manager_initialized(game_config: &InstantGameConfig) -> Result<()> {
    if !game_config.is_initialized() {
        //TODO: this should be a print
        FzfWrapper::message("❌ Error: Game manager is not initialized.\n\nPlease run 'instant game init' first.")
            .context("Failed to show uninitialized message")?;
        return Err(anyhow::anyhow!("game manager not initialized"));
    }
    Ok(())
}

/// Check both restic availability and game manager initialization
pub fn check_restic_and_game_manager(game_config: &InstantGameConfig) -> Result<()> {
    check_restic_availability()?;
    check_game_manager_initialized(game_config)?;
    Ok(())
}
