pub mod backup;
pub mod commands;
pub mod snapshot_selection;

use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::selection;
use anyhow::{Context, Result};

/// Handle game save backup with optional game selection
pub fn backup_game_saves(game_name: Option<String>) -> Result<()> {
    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Check restic availability and game manager initialization
    super::utils::validation::check_restic_and_game_manager(&game_config)?;

    // Get game name
    let game_name = match game_name {
        Some(name) => name,
        None => match selection::select_game_interactive(None)? {
            Some(name) => name,
            None => return Ok(()),
        },
    };

    // Find the game installation
    let installation = match installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name)
    {
        Some(installation) => installation,
        None => {
            FzfWrapper::message(&format!(
                "❌ Error: No installation found for game '{game_name}'.\n\nPlease add the game first using 'instant game add'."
            )).context("Failed to show game not found message")?;
            return Err(anyhow::anyhow!("game installation not found"));
        }
    };

    // Security check: ensure save directory is not empty
    let save_path = installation.save_path.as_path();
    if !save_path.exists() {
        FzfWrapper::message(&format!(
            "❌ Error: Save path does not exist for game '{}': {}\n\nPlease check the game installation configuration.",
            game_name,
            save_path.display()
        )).context("Failed to show save path not found message")?;
        return Err(anyhow::anyhow!("save path does not exist"));
    }

    // Check if save directory is empty
    let mut is_empty = true;
    if let Ok(mut entries) = std::fs::read_dir(save_path) {
        if let Some(entry) = entries.next() {
            // Only consider non-hidden files/directories
            if let Ok(entry) = entry {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                if !file_name_str.starts_with('.') {
                    is_empty = false;
                }
            }
        }
    }

    if is_empty {
        FzfWrapper::message(&format!(
            "❌ Security: Refusing to backup empty save directory for game '{}': {}\n\nThe save directory appears to be empty or contains only hidden files. This could indicate:\n• The game has not created any saves yet\n• The save path is configured incorrectly\n• The saves are stored in a different location\n\nPlease verify the save path configuration and ensure the game has created save files.",
            game_name,
            save_path.display()
        )).context("Failed to show empty directory warning")?;
        return Err(anyhow::anyhow!(
            "save directory is empty - security precaution"
        ));
    }

    // Create backup
    let backup_handler = backup::GameBackup::new(game_config);

    println!(
        "🔄 Creating backup for '{game_name}'...\nThis may take a while depending on save file size."
    );

    match backup_handler.backup_game(installation) {
        Ok(output) => {
            println!("✅ Backup completed successfully for game '{game_name}'!\n\n{output}");
        }
        Err(e) => {
            eprintln!("❌ Backup failed for game '{game_name}': {e}");
            return Err(e);
        }
    }

    Ok(())
}

/// Handle game save restore with optional game and snapshot selection
pub fn restore_game_saves(game_name: Option<String>, snapshot_id: Option<String>) -> Result<()> {
    use crate::game::config::InstallationsConfig;
    use crate::game::restic::backup::GameBackup;
    use crate::game::restic::snapshot_selection::select_snapshot_interactive;

    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Check restic availability and game manager initialization
    super::utils::validation::check_restic_and_game_manager(&game_config)?;

    // Get game name
    let game_name = match game_name {
        Some(name) => name,
        None => match selection::select_game_interactive(None)? {
            Some(name) => name,
            None => return Ok(()),
        },
    };

    // Find the game installation
    let installation = match installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name)
    {
        Some(installation) => installation,
        None => {
            FzfWrapper::message(&format!(
                "❌ Error: No installation found for game '{game_name}'.\n\nPlease add the game first using 'instant game add'."
            )).context("Failed to show game not found message")?;
            return Err(anyhow::anyhow!("game installation not found"));
        }
    };

    // Get snapshot ID
    let snapshot_id = match snapshot_id {
        Some(id) => id,
        None => match select_snapshot_interactive(&game_name)? {
            Some(id) => id,
            None => return Ok(()),
        },
    };

    // Validate save path exists
    let save_path = installation.save_path.as_path();
    if !save_path.exists() {
        FzfWrapper::message(&format!(
            "❌ Error: Save path does not exist for game '{}': {}\n\nThe save directory will be created during restore.",
            game_name,
            save_path.display()
        )).context("Failed to show save path not found message")?;
        // Continue with restore - the directory will be created
    }

    // Create backup handler
    let backup_handler = GameBackup::new(game_config);

    // Show restore confirmation
    let confirmed = FzfWrapper::confirm_builder()
        .message(&format!(
            "🔄 Restore game saves for '{}' from snapshot {}\n\n⚠️  This will overwrite existing save files in:\n   {}\n\nThis action cannot be undone. Are you sure you want to continue?",
            game_name,
            &snapshot_id[..8.min(snapshot_id.len())], // Show first 8 characters of snapshot ID
            save_path.display()
        ))
        .yes_text("Restore")
        .no_text("Cancel")
        .show()
        .context("Failed to show restore confirmation dialog")?;

    if confirmed != crate::fzf_wrapper::ConfirmResult::Yes {
        FzfWrapper::message("Restore cancelled by user.")
            .context("Failed to show cancellation message")?;
        return Ok(());
    }

    // Perform the restore
    println!("🔄 Restoring game saves for '{}'...", game_name);

    match backup_handler.restore_game_backup(&game_name, &snapshot_id, save_path) {
        Ok(output) => {
            println!("✅ Restore completed successfully for game '{}'!\n\n{}", game_name, output);
        }
        Err(e) => {
            eprintln!("❌ Restore failed for game '{}': {}", game_name, e);
            return Err(e);
        }
    }

    Ok(())
}

/// Handle restic command passthrough with instant games repository configuration
pub fn handle_restic_command(args: Vec<String>) -> Result<()> {
    // Load configuration
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;

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
    cmd.arg("-r").arg(game_config.repo.as_path());

    // Set password via environment variable
    cmd.env("RESTIC_PASSWORD", &game_config.repo_password);

    // Add user-provided arguments
    cmd.args(&args);

    // Execute the command
    let output = cmd.output().context("Failed to execute restic command")?;

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
