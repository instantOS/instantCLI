pub mod backup;
pub mod commands;

use anyhow::{Context, Result};
use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::{InstantGameConfig, InstallationsConfig};
use crate::game::games::selection;

/// Handle game save backup with optional game selection
pub fn backup_game_saves(game_name: Option<String>) -> Result<()> {
    // Load configurations
    let game_config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;
    let installations = InstallationsConfig::load()
        .context("Failed to load installations configuration")?;

    // Check if restic is available
    // TODO: Other parts of the codebase will also use restic, so this error check and message
    // should be moved to a more general location
    if !backup::GameBackup::check_restic_availability()? {
        FzfWrapper::message("❌ Error: restic is not installed or not found in PATH.\n\nPlease install restic to use backup functionality.")
            .context("Failed to show restic not available message")?;
        return Err(anyhow::anyhow!("restic not available"));
    }

    // Check if game manager is initialized
    // TODO: Other parts of the codebase will also use the game manager, so this error check and message
    // should be moved to a more general location
    if !game_config.is_initialized() {
        FzfWrapper::message("❌ Error: Game manager is not initialized.\n\nPlease run 'instant game init' first.")
            .context("Failed to show uninitialized message")?;
        return Err(anyhow::anyhow!("game manager not initialized"));
    }

    // Get game name
    let game_name = match game_name {
        Some(name) => name,
        None => {
            match selection::select_game_interactive(None)? {
                Some(name) => name,
                None => return Ok(()),
            }
        }
    };

    // Find the game installation
    let installation = match installations.installations.iter()
        .find(|inst| inst.game_name.0 == game_name) {
        Some(installation) => installation,
        None => {
            FzfWrapper::message(&format!(
                "❌ Error: No installation found for game '{}'.\n\nPlease add the game first using 'instant game add'.",
                game_name
            )).context("Failed to show game not found message")?;
            return Err(anyhow::anyhow!("game installation not found"));
        }
    };

    // Create backup
    let backup_handler = backup::GameBackup::new(game_config);

    FzfWrapper::message(&format!(
        "🔄 Creating backup for '{}'...\nThis may take a while depending on save file size.",
        game_name
    )).context("Failed to show backup started message")?;

    match backup_handler.backup_game(installation) {
        Ok(output) => {
            FzfWrapper::message_builder()
                .message(format!(
                    "✅ Backup completed successfully for game '{}'!\n\n{}",
                    game_name, output
                ))
                .title("Backup Complete")
                .show()
                .context("Failed to show backup success message")?;
        }
        Err(e) => {
            FzfWrapper::message(&format!(
                "❌ Backup failed for game '{}': {}",
                game_name, e
            )).context("Failed to show backup failure message")?;
            return Err(e);
        }
    }

    Ok(())
}
