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

    // Check restic availability and game manager initialization
    super::utils::validation::check_restic_and_game_manager(&game_config)?;

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
            println!(
                "✅ Backup completed successfully for game '{}'!\n\n{}",
                game_name, output
            );
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

    // Check restic availability and game manager initialization
    super::utils::validation::check_restic_and_game_manager(&game_config)?;

    // If no arguments provided, show help
    if args.is_empty() {
        eprintln!(
            "❌ Error: No restic command provided.\n\n\
             Usage: instant game restic <restic-command> [args...]\n\n\
             Examples:\n\
             • instant game restic snapshots\n\
             • instant game restic backup --tag instantgame\n\
             • instant game restic stats\n\
             • instant game restic find .config\n\
             • instant game restic restore latest --target /tmp/restore-test"
        );
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
