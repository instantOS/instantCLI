use crate::game::config::{GameInstallation, PathContentKind};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::Colorize;

pub(crate) fn emit_restic_event(
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

pub(crate) fn validate_backup_requirements(
    game_name: &str,
    installation: &GameInstallation,
) -> Result<()> {
    let save_path = installation.save_path.as_path();
    if !save_path.exists() {
        let path_display = save_path.display().to_string();
        emit_restic_event(
            Level::Error,
            "game.backup.save_path_missing",
            Some(char::from(NerdFont::CrossCircle)),
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
                "game": game_name,
                "action": "save_path_missing",
                "path": path_display
            })),
        );
        emit_restic_event(
            Level::Warn,
            "game.backup.hint.config",
            Some(char::from(NerdFont::Warning)),
            "Please check the game installation configuration.".to_string(),
            "Please check the game installation configuration.".to_string(),
            Some(serde_json::json!({
                "hint": "check_configuration"
            })),
        );
        return Err(anyhow::anyhow!("save path does not exist"));
    }

    // Check if save path is empty based on type
    let is_empty = match installation.save_path_type {
        PathContentKind::File => {
            // For files, check if file is empty
            save_path
                .metadata()
                .map_or(true, |metadata| metadata.len() == 0)
        }
        PathContentKind::Directory => {
            // For directories, check if directory is empty (ignoring hidden files)
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
            is_empty
        }
    };

    if is_empty {
        let path_display = save_path.display().to_string();
        let (entity_type, error_msg) = match installation.save_path_type {
            PathContentKind::File => ("save file", "save file is empty - security precaution"),
            PathContentKind::Directory => (
                "save directory",
                "save directory is empty - security precaution",
            ),
        };

        emit_restic_event(
            Level::Error,
            "game.backup.security.empty_path",
            Some(char::from(NerdFont::CrossCircle)),
            format!(
                "Security: Refusing to backup empty {} for game '{}': {}",
                entity_type, game_name, path_display
            ),
            format!(
                "Security: Refusing to backup empty {} for game '{}': {}",
                entity_type,
                game_name.red(),
                path_display.red()
            ),
            Some(serde_json::json!({
                "game": game_name,
                "action": "empty_save_path",
                "path": path_display,
                "path_type": entity_type
            })),
        );

        emit_restic_event(
            Level::Info,
            "game.backup.security.context",
            Some(char::from(NerdFont::Info)),
            format!(
                "The {} appears to be empty or contains only hidden files. This could indicate:",
                entity_type
            ),
            format!(
                "The {} appears to be empty or contains only hidden files. This could indicate:",
                entity_type
            ),
            Some(serde_json::json!({
                "context": "empty_save_path",
                "path_type": entity_type
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
                    "context": "empty_save_path",
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
                "context": "empty_save_path",
                "action": "verify_save_path"
            })),
        );
        return Err(anyhow::anyhow!(error_msg));
    }

    Ok(())
}
