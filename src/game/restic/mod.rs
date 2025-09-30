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
use colored::Colorize;

fn emit_restic_event(
    level: Level,
    code: &str,
    icon: Option<char>,
    plain_message: impl Into<String>,
    text_message: impl Into<String>,
    data: Option<serde_json::Value>,
) {
    let plain = plain_message.into();
    let text = text_message.into();
    let formatted = if matches!(get_output_format(), OutputFormat::Json) {
        plain
    } else if let Some(icon) = icon {
        format!("{icon} {text}")
    } else {
        text
    };
    emit(level, code, &formatted, data);
}

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
            let plain_message = format!("Error: No installation found for game '{game_name}'.");
            let text_message = format!(
                "Error: No installation found for game '{}'.",
                game_name.red()
            );
            let data = serde_json::json!({
                "game": game_name,
                "action": "installation_missing"
            });
            emit_restic_event(
                Level::Error,
                "game.backup.installation_missing",
                Some(char::from(Fa::TimesCircle)),
                plain_message,
                text_message,
                Some(data),
            );
            emit_restic_event(
                Level::Info,
                "game.backup.hint.add",
                Some(char::from(Fa::InfoCircle)),
                format!(
                    "Please add the game first using '{} game add'.",
                    env!("CARGO_BIN_NAME")
                ),
                format!(
                    "Please add the game first using '{} game add'.",
                    env!("CARGO_BIN_NAME")
                ),
                Some(serde_json::json!({
                    "hint": "add_game",
                    "command": format!("{} game add", env!("CARGO_BIN_NAME"))
                })),
            );
            return Err(anyhow::anyhow!("game installation not found"));
        }
    };

    // Security check: ensure save directory is not empty
    let save_path = installation.save_path.as_path();
    if !save_path.exists() {
        let path_display = save_path.display().to_string();
        emit_restic_event(
            Level::Error,
            "game.backup.save_path_missing",
            Some(char::from(Fa::TimesCircle)),
            format!(
                "Error: Save path does not exist for game '{}': {}",
                game_name, path_display
            ),
            format!(
                "Error: Save path does not exist for game '{}': {}",
                game_name.red(),
                path_display
            ),
            Some(serde_json::json!({
                "game": game_name.clone(),
                "action": "save_path_missing",
                "path": path_display
            })),
        );
        emit_restic_event(
            Level::Warn,
            "game.backup.hint.config",
            Some(char::from(Fa::ExclamationCircle)),
            "Please check the game installation configuration.".to_string(),
            "Please check the game installation configuration.".to_string(),
            Some(serde_json::json!({
                "hint": "check_configuration"
            })),
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
        let path_display = save_path.display().to_string();
        emit_restic_event(
            Level::Error,
            "game.backup.security.empty_dir",
            Some(char::from(Fa::TimesCircle)),
            format!(
                "Security: Refusing to backup empty save directory for game '{}': {}",
                game_name, path_display
            ),
            format!(
                "Security: Refusing to backup empty save directory for game '{}': {}",
                game_name.red(),
                path_display
            ),
            Some(serde_json::json!({
                "game": game_name.clone(),
                "action": "empty_save_directory",
                "path": path_display
            })),
        );

        emit_restic_event(
            Level::Info,
            "game.backup.security.context",
            Some(char::from(Fa::InfoCircle)),
            "The save directory appears to be empty or contains only hidden files. This could indicate:".to_string(),
            "The save directory appears to be empty or contains only hidden files. This could indicate:".to_string(),
            Some(serde_json::json!({
                "context": "empty_save_directory"
            })),
        );

        let reasons = [
            (
                "game.backup.security.reason1",
                "The game has not created any saves yet",
                "no_visible_saves",
            ),
            (
                "game.backup.security.reason2",
                "The save path is configured incorrectly",
                "save_path_incorrect",
            ),
            (
                "game.backup.security.reason3",
                "The saves are stored in a different location",
                "saves_elsewhere",
            ),
        ];

        for (code, text, key) in reasons {
            emit_restic_event(
                Level::Info,
                code,
                None,
                text.to_string(),
                format!("• {text}"),
                Some(serde_json::json!({
                    "context": "empty_save_directory",
                    "detail": key
                })),
            );
        }

        emit_restic_event(
            Level::Info,
            "game.backup.security.action",
            None,
            "Please verify the save path configuration and ensure the game has created save files."
                .to_string(),
            "Please verify the save path configuration and ensure the game has created save files."
                .to_string(),
            Some(serde_json::json!({
                "context": "empty_save_directory",
                "action": "verify_save_path"
            })),
        );
        return Err(anyhow::anyhow!(
            "save directory is empty - security precaution"
        ));
    }

    // Create backup
    let backup_handler = backup::GameBackup::new(game_config.clone());

    emit_restic_event(
        Level::Info,
        "game.backup.start",
        Some(char::from(Fa::Save)),
        format!(
            "Creating backup for '{}'... This may take a while depending on save file size.",
            game_name
        ),
        format!(
            "Creating backup for '{}'...\nThis may take a while depending on save file size.",
            game_name.yellow()
        ),
        Some(serde_json::json!({
            "game": game_name.clone(),
            "action": "backup_start"
        })),
    );

    match backup_handler.backup_game(installation) {
        Ok(output) => {
            emit_restic_event(
                Level::Success,
                "game.backup.completed",
                Some(char::from(Fa::Check)),
                format!("Backup completed successfully for game '{game_name}'!"),
                format!(
                    "Backup completed successfully for game '{}'!\n\n{}",
                    game_name.green(),
                    output
                ),
                Some(serde_json::json!({
                    "game": game_name.clone(),
                    "action": "backup_completed",
                    "output": output
                })),
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
            emit_restic_event(
                Level::Error,
                "game.backup.failed",
                Some(char::from(Fa::TimesCircle)),
                format!("Backup failed for game '{game_name}': {e}"),
                format!("Backup failed for game '{}': {}", game_name.red(), e),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "backup_failed",
                    "error": e.to_string()
                })),
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

    let game_name_plain = game_selection.game_name.clone();

    // Step 4: Check if restore should be skipped due to matching checkpoint
    if !force
        && let Some(ref nearest_checkpoint) = game_selection.installation.nearest_checkpoint
        && nearest_checkpoint == &snapshot_id
    {
        let plain_message = format!(
            "Restore skipped for game '{}' from snapshot {} (checkpoint matches, use --force to override)",
            game_name_plain, snapshot_id
        );
        let text_message = format!(
            "Restore skipped for game '{}' from snapshot {} (checkpoint matches, use --force to override)",
            game_selection.game_name.yellow(),
            snapshot_id
        );
        let snapshot_for_data = snapshot_id.clone();
        emit_restic_event(
            Level::Info,
            "game.restore.skipped",
            Some(char::from(Fa::InfoCircle)),
            plain_message,
            text_message,
            Some(serde_json::json!({
                "game": game_name_plain.clone(),
                "action": "restore_skipped",
                "snapshot_id": snapshot_for_data,
                "reason": "checkpoint_matches"
            })),
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
                let plain_message = format!(
                    "Error: Snapshot '{}' not found for game '{}'.",
                    snapshot_id, game_name_plain
                );
                let text_message = format!(
                    "Error: Snapshot '{}' not found for game '{}'.",
                    snapshot_id,
                    game_selection.game_name.red()
                );
                emit_restic_event(
                    Level::Error,
                    "game.restore.snapshot_missing",
                    Some(char::from(Fa::TimesCircle)),
                    plain_message,
                    text_message,
                    Some(serde_json::json!({
                        "game": game_name_plain.clone(),
                        "action": "snapshot_missing",
                        "snapshot_id": snapshot_id
                    })),
                );
                emit_restic_event(
                    Level::Info,
                    "game.restore.hint.snapshot",
                    Some(char::from(Fa::InfoCircle)),
                    "Please select a valid snapshot.".to_string(),
                    "Please select a valid snapshot.".to_string(),
                    Some(serde_json::json!({
                        "hint": "select_valid_snapshot"
                    })),
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
        emit_restic_event(
            Level::Warn,
            "game.restore.cancelled.security",
            Some(char::from(Fa::ExclamationCircle)),
            "Restore cancelled due to security warning.".to_string(),
            "Restore cancelled due to security warning.".to_string(),
            Some(serde_json::json!({
                "game": game_name_plain.clone(),
                "action": "restore_cancelled_security"
            })),
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
        emit_restic_event(
            Level::Warn,
            "game.restore.cancelled.user",
            Some(char::from(Fa::ExclamationCircle)),
            "Restore cancelled by user.".to_string(),
            "Restore cancelled by user.".to_string(),
            Some(serde_json::json!({
                "game": game_name_plain.clone(),
                "action": "restore_cancelled_user"
            })),
        );
        return Ok(());
    }

    // Step 8: Perform the restore
    let save_path = game_selection.installation.save_path.as_path();
    let backup_handler = GameBackup::new(game_config);

    emit_restic_event(
        Level::Info,
        "game.restore.start",
        Some(char::from(Fa::Download)),
        format!("Restoring game saves for '{}'...", game_name_plain),
        format!(
            "Restoring game saves for '{}'...",
            game_selection.game_name.yellow()
        ),
        Some(serde_json::json!({
            "game": game_selection.game_name.clone(),
            "action": "restore_start",
            "snapshot_id": snapshot_id.clone()
        })),
    );

    match backup_handler.restore_game_backup(&game_selection.game_name, &snapshot_id, save_path) {
        Ok(output) => {
            let output_clone = output.clone();
            emit_restic_event(
                Level::Success,
                "game.restore.completed",
                Some(char::from(Fa::Check)),
                format!(
                    "Restore completed successfully for game '{}'!",
                    game_selection.game_name
                ),
                format!(
                    "Restore completed successfully for game '{}'!\n\n{}",
                    game_selection.game_name.green(),
                    output
                ),
                Some(serde_json::json!({
                    "game": game_selection.game_name.clone(),
                    "action": "restore_completed",
                    "snapshot_id": snapshot_id.clone(),
                    "output": output_clone
                })),
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
            emit_restic_event(
                Level::Error,
                "game.restore.failed",
                Some(char::from(Fa::TimesCircle)),
                format!(
                    "Restore failed for game '{}': {}",
                    game_selection.game_name, e
                ),
                format!(
                    "Restore failed for game '{}': {}",
                    game_selection.game_name.red(),
                    e
                ),
                Some(serde_json::json!({
                    "game": game_selection.game_name.clone(),
                    "action": "restore_failed",
                    "snapshot_id": snapshot_id,
                    "error": e.to_string()
                })),
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
