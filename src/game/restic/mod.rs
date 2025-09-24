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
            //TODO: change this to be a print instead of a popup. 
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

/// Handle restic command passthrough with instant games repository configuration
pub fn handle_restic_command(args: Vec<String>) -> Result<()> {
    // Load configuration
    let game_config = InstantGameConfig::load()
        .context("Failed to load game configuration")?;

    // Check if restic is available
    if !backup::GameBackup::check_restic_availability()? {
        FzfWrapper::message("❌ Error: restic is not installed or not found in PATH.\n\nPlease install restic to use restic commands.")
            .context("Failed to show restic not available message")?;
        return Err(anyhow::anyhow!("restic not available"));
    }

    // Check if game manager is initialized
    if !game_config.is_initialized() {
        FzfWrapper::message("❌ Error: Game manager is not initialized.\n\nPlease run 'instant game init' first.")
            .context("Failed to show uninitialized message")?;
        return Err(anyhow::anyhow!("game manager not initialized"));
    }

    // If no arguments provided, show help
    if args.is_empty() {
        FzfWrapper::message(
            "❌ Error: No restic command provided.\n\n\
             Usage: instant game restic <restic-command> [args...]\n\n\
             Examples:\n\
             • instant game restic snapshots\n\
             • instant game restic backup --tag instantgame\n\
             • instant game restic stats\n\
             • instant game restic find .config\n\
             • instant game restic restore latest --target /tmp/restore-test"
        ).context("Failed to show restic help message")?;
        return Err(anyhow::anyhow!("no restic command provided"));
    }

    // Build restic command with repository and password
    let mut cmd = std::process::Command::new("restic");

    // Set repository
    cmd.arg("-r")
       .arg(game_config.repo.as_path());

    // Set password via environment variable
    cmd.env("RESTIC_PASSWORD", &game_config.repo_password);

    // Add user-provided arguments
    cmd.args(&args);

    // Execute the command
    let output = cmd.output()
        .context("Failed to execute restic command")?;

    // Print stdout to the user
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    // Print stderr to the user
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    // Return appropriate result based on exit code
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "restic command failed with exit code: {}",
            output.status.code().unwrap_or(-1)
        ))
    }
}
