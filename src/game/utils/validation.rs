use crate::game::config::InstantGameConfig;
use crate::game::restic::backup;
use anyhow::Result;

/// Common validation utilities for game manager operations
/// Check if restic is available and show error message if not
pub fn check_restic_availability() -> Result<()> {
    if !backup::GameBackup::check_restic_availability()? {
        eprintln!(
            "❌ Error: restic is not installed or not found in PATH.\n\nPlease install restic to use backup functionality."
        );
        return Err(anyhow::anyhow!("restic not available"));
    }
    Ok(())
}

/// Check if game manager is initialized and show error message if not
pub fn check_game_manager_initialized(game_config: &InstantGameConfig) -> Result<()> {
    if !game_config.is_initialized() {
        eprintln!(
            "❌ Error: Game manager is not initialized.\n\nPlease run 'instant game init' first."
        );
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
