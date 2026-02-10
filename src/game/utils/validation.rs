use crate::game::config::InstantGameConfig;
use crate::game::restic::backup;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

/// Common validation utilities for game manager operations
/// Check if restic is available and show error message if not
pub fn check_restic_availability() -> Result<()> {
    if !backup::GameBackup::check_restic_availability()? {
        eprintln!(
            "{} Error: restic is not installed or not found in PATH.\n\nPlease install restic to use backup functionality.",
            char::from(NerdFont::CrossCircle)
        );
        return Err(anyhow::anyhow!("restic not available"));
    }
    Ok(())
}

/// Check if game manager is initialized and show error message if not
pub fn check_game_manager_initialized(game_config: &InstantGameConfig) -> Result<()> {
    if !game_config.is_initialized() {
        eprintln!(
            "{} Error: Game manager is not initialized.\n\nPlease run '{} game init' first.",
            char::from(NerdFont::CrossCircle),
            env!("CARGO_BIN_NAME")
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

/// Prompt user to initialize game manager if not initialized
/// Returns Ok(true) if initialized or user initialized successfully
/// Returns Ok(false) if user declined or initialization failed
pub fn prompt_initialize_if_needed() -> Result<bool> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    if config.is_initialized() {
        return Ok(true);
    }

    let init = FzfWrapper::builder()
        .confirm("Game save manager is not initialized.\n\nInitialize it now to start managing game saves with restic backups?")
        .yes_text("Yes, initialize now")
        .no_text("No, cancel")
        .confirm_dialog()?;

    if init == ConfirmResult::Yes {
        // Call initialization logic
        crate::game::repository::manager::GameRepositoryManager::initialize_game_manager(
            false,
            Default::default(),
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}
