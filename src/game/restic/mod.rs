pub mod backup;
pub mod cache;
pub mod commands;
pub mod prune;
pub mod security;
pub mod snapshot_selection;
pub mod tags;

use crate::game::checkpoint;
use crate::game::config::{InstallationsConfig, InstantGameConfig};
use crate::game::games::selection;
use crate::ui::prelude::*;
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
            emit(
                Level::Error,
                "game.backup.installation_missing",
                &format!(
                    "{} Error: No installation found for game '{}'.",
                    char::from(Fa::TimesCircle),
                    game_name
                ),
                None,
            );
            emit(
                Level::Info,
                "game.backup.hint.add",
                &format!(
                    "{} Please add the game first using '{} game add'.",
                    char::from(Fa::InfoCircle),
                    env!("CARGO_BIN_NAME")
                ),
                None,
            );
            return Err(anyhow::anyhow!("game installation not found"));
        }
    };

    // Security check: ensure save directory is not empty
    let save_path = installation.save_path.as_path();
    if !save_path.exists() {
        emit(
            Level::Error,
            "game.backup.save_path_missing",
            &format!(
                "{} Error: Save path does not exist for game '{}': {}",
                char::from(Fa::TimesCircle),
                game_name,
                save_path.display()
            ),
            None,
        );
        emit(
            Level::Warn,
            "game.backup.hint.config",
            &format!(
                "{} Please check the game installation configuration.",
                char::from(Fa::ExclamationCircle)
            ),
            None,
        );
        return Err(anyhow::anyhow!("save path does not exist"));
    }

    // Check if save directory is empty
    let mut is_empty = true;
    if let Ok(mut entries) = std::fs::read_dir(save_path)
        && let Some(entry) = entries.next()
    {
        // Only consider non-hidden files/directories
        if let Ok(entry) = entry {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();
            if !file_name_str.starts_with('.') {
                is_empty = false;
            }
        }
    }

    if is_empty {
        emit(
            Level::Error,
            "game.backup.security.empty_dir",
            &format!(
                "{} Security: Refusing to backup empty save directory for game '{}': {}",
                char::from(Fa::TimesCircle),
                game_name,
                save_path.display()
            ),
            None,
        );
        emit(
            Level::Info,
            "game.backup.security.context",
            &format!(
                "{} The save directory appears to be empty or contains only hidden files. This could indicate:",
                char::from(Fa::InfoCircle)
            ),
            None,
        );
        emit(
            Level::Info,
            "game.backup.security.reason1",
            "• The game has not created any saves yet",
            None,
        );
        emit(
            Level::Info,
            "game.backup.security.reason2",
            "• The save path is configured incorrectly",
            None,
        );
        emit(
            Level::Info,
            "game.backup.security.reason3",
            "• The saves are stored in a different location",
            None,
        );
        emit(
            Level::Info,
            "game.backup.security.action",
            "Please verify the save path configuration and ensure the game has created save files.",
            None,
        );
        return Err(anyhow::anyhow!(
            "save directory is empty - security precaution"
        ));
    }

    // Create backup
    let backup_handler = backup::GameBackup::new(game_config.clone());

    emit(
        Level::Info,
        "game.backup.start",
        &format!(
            "{} Creating backup for '{}'...\nThis may take a while depending on save file size.",
            char::from(Fa::Save),
            game_name
        ),
        None,
    );

    match backup_handler.backup_game(installation) {
        Ok(output) => {
            emit(
                Level::Success,
                "game.backup.completed",
                &format!(
                    "{} Backup completed successfully for game '{}'!\n\n{}",
                    char::from(Fa::Check),
                    game_name,
                    output
                ),
                None,
            );

            // Update checkpoint after successful backup
            if let Err(e) =
                checkpoint::update_checkpoint_after_backup(&output, &game_name, &game_config)
            {
                eprintln!("Warning: Could not update checkpoint: {e}");
            }

            // Invalidate cache for this game after successful backup
            let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
            cache::invalidate_game_cache(&game_name, &repo_path);
        }
        Err(e) => {
            emit(
                Level::Error,
                "game.backup.failed",
                &format!(
                    "{} Backup failed for game '{game_name}': {}",
                    char::from(Fa::TimesCircle),
                    e
                ),
                None,
            );
            return Err(e);
        }
    }

    Ok(())
}

/// Handle game save restore with optional game and snapshot selection
pub fn restore_game_saves(
    game_name: Option<String>,
    snapshot_id: Option<String>,
    force: bool,
) -> Result<()> {
    use crate::game::restic::backup::GameBackup;
    use crate::game::restic::snapshot_selection::select_snapshot_interactive_with_local_comparison;

    // Step 1: Game selection using security module
    let game_selection = security::get_game_installation(game_name)?;
    if game_selection.game_name.is_empty() {
        // User cancelled selection
        return Ok(());
    }

    // Step 2: Validate restore environment
    let security_result = security::validate_restore_environment(&game_selection.installation)?;

    // Step 3: Get snapshot ID with enhanced selection (includes local save comparison)
    let snapshot_id = match snapshot_id {
        Some(id) => {
            // Validate the provided snapshot ID exists
            let game_config = InstantGameConfig::load()
                .context("Failed to load game configuration for snapshot validation")?;
            if !security::validate_snapshot_id(&id, &game_selection.game_name, &game_config)? {
                return Ok(());
            }
            id
        }
        None => {
            // Use enhanced snapshot selection with local save comparison
            match select_snapshot_interactive_with_local_comparison(
                &game_selection.game_name,
                Some(&game_selection.installation),
            )? {
                Some(id) => id,
                None => return Ok(()),
            }
        }
    };

    // Step 4: Check if restore should be skipped due to matching checkpoint
    if !force
        && let Some(ref nearest_checkpoint) = game_selection.installation.nearest_checkpoint
        && nearest_checkpoint == &snapshot_id
    {
        emit(
            Level::Info,
            "game.restore.skipped",
            &format!(
                "{} Restore skipped for game '{}' from snapshot {} (checkpoint matches, use --force to override)",
                char::from(Fa::InfoCircle),
                game_selection.game_name,
                snapshot_id
            ),
            None,
        );
        return Ok(());
    }

    // Step 5: Get snapshot details for security checks
    let game_config =
        InstantGameConfig::load().context("Failed to load game configuration for restore")?;
    let snapshot =
        match cache::get_snapshot_by_id(&snapshot_id, &game_selection.game_name, &game_config)? {
            Some(snapshot) => snapshot,
            None => {
                emit(
                    Level::Error,
                    "game.restore.snapshot_missing",
                    &format!(
                        "{} Error: Snapshot '{}' not found for game '{}'.",
                        char::from(Fa::TimesCircle),
                        snapshot_id,
                        game_selection.game_name
                    ),
                    None,
                );
                emit(
                    Level::Info,
                    "game.restore.hint.snapshot",
                    &format!(
                        "{} Please select a valid snapshot.",
                        char::from(Fa::InfoCircle)
                    ),
                    None,
                );
                return Err(anyhow::anyhow!("snapshot not found"));
            }
        };

    // Step 6: Perform security check for snapshot vs local saves
    if let Some(ref save_info) = security_result.save_info
        && !security::check_snapshot_vs_local_saves(
            &snapshot,
            save_info,
            &game_selection.game_name,
            force,
        )?
    {
        // User cancelled due to security warning
        emit(
            Level::Warn,
            "game.restore.cancelled.security",
            &format!(
                "{} Restore cancelled due to security warning.",
                char::from(Fa::ExclamationCircle)
            ),
            None,
        );
        return Ok(());
    }

    // Step 7: Show enhanced restore confirmation with security information
    if !security::create_restore_confirmation(
        &game_selection.game_name,
        &snapshot,
        &security_result,
        force,
    )? {
        // User cancelled confirmation
        emit(
            Level::Warn,
            "game.restore.cancelled.user",
            &format!(
                "{} Restore cancelled by user.",
                char::from(Fa::ExclamationCircle)
            ),
            None,
        );
        return Ok(());
    }

    // Step 8: Perform the restore
    let save_path = game_selection.installation.save_path.as_path();
    let backup_handler = GameBackup::new(game_config);

    emit(
        Level::Info,
        "game.restore.start",
        &format!(
            "{} Restoring game saves for '{}'...",
            char::from(Fa::Download),
            game_selection.game_name
        ),
        None,
    );

    match backup_handler.restore_game_backup(&game_selection.game_name, &snapshot_id, save_path) {
        Ok(output) => {
            emit(
                Level::Success,
                "game.restore.completed",
                &format!(
                    "{} Restore completed successfully for game '{}'!\n\n{}",
                    char::from(Fa::Check),
                    game_selection.game_name,
                    output
                ),
                None,
            );

            // Update the installation with the checkpoint
            checkpoint::update_checkpoint_after_restore(&game_selection.game_name, &snapshot_id)?;

            // Invalidate cache for this game after successful restore
            let repo_path = backup_handler
                .config
                .repo
                .as_path()
                .to_string_lossy()
                .to_string();
            cache::invalidate_game_cache(&game_selection.game_name, &repo_path);
        }
        Err(e) => {
            emit(
                Level::Error,
                "game.restore.failed",
                &format!(
                    "{} Restore failed for game '{}': {}",
                    char::from(Fa::TimesCircle),
                    game_selection.game_name,
                    e
                ),
                None,
            );
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
        let bin = env!("CARGO_BIN_NAME");
        eprintln!(
            "Error: No restic command provided.\n\n\
             Usage: {bin} game restic <restic-command> [args...]\n\n\
             Examples:\n\
             • {bin} game restic snapshots\n\
             • {bin} game restic backup --tag instantgame\n\
             • {bin} game restic stats\n\
             • {bin} game restic find .config\n\
             • {bin} game restic restore latest --target /tmp/restore-test",
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

/// Prune game snapshots using the configured strategy
pub fn prune_snapshots(game_name: Option<String>, zero_changes: bool) -> Result<()> {
    prune::prune_snapshots(game_name, zero_changes)
}
