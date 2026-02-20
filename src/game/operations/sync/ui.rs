use super::types::ToleranceDirection;
use crate::game::utils::save_files::SYNC_TOLERANCE_SECONDS;
use crate::ui::prelude::*;
use colored::*;

pub fn emit_with_icon(
    level: Level,
    code: &str,
    icon: char,
    plain_message: impl Into<String>,
    text_message: impl Into<String>,
    data: Option<serde_json::Value>,
) {
    let plain = plain_message.into();
    let text = text_message.into();
    let formatted = if matches!(get_output_format(), OutputFormat::Json) {
        plain
    } else {
        format!("{icon} {text}")
    };
    emit(level, code, &formatted, data);
}

pub fn emit_separator() {
    let ch = if matches!(get_output_format(), OutputFormat::Json) {
        '-'
    } else {
        '─'
    };
    let line: String = std::iter::repeat_n(ch, 80).collect();
    emit(Level::Info, "separator", &line, None);
}

fn format_delta(delta_seconds: i64) -> String {
    let secs = delta_seconds.unsigned_abs();
    if secs < 60 {
        format!("{} seconds", secs)
    } else {
        let minutes = secs / 60;
        let seconds = secs % 60;
        if seconds == 0 {
            format!("{} minutes", minutes)
        } else {
            format!("{} minutes {} seconds", minutes, seconds)
        }
    }
}

fn format_tolerance_window() -> String {
    let secs = SYNC_TOLERANCE_SECONDS.unsigned_abs();
    if secs < 60 {
        format!("{} seconds", secs)
    } else {
        let minutes = secs / 60;
        let seconds = secs % 60;
        if seconds == 0 {
            format!("{} minutes", minutes)
        } else {
            format!("{} minutes {} seconds", minutes, seconds)
        }
    }
}

pub fn report_no_action_needed(game_name: &str) {
    emit_with_icon(
        Level::Success,
        "game.sync.already_in_sync",
        char::from(NerdFont::Check),
        format!("{}: Already in sync", game_name),
        format!("{}: Already in sync", game_name.green()),
        Some(serde_json::json!({
            "game": game_name,
            "action": "already_in_sync"
        })),
    );
}

pub fn report_within_tolerance(game_name: &str, direction: ToleranceDirection, delta_seconds: i64) {
    let delta_str = format_delta(delta_seconds);
    let tolerance_str = format_tolerance_window();
    let (plain_msg, text_msg, code, direction_value) = match direction {
        ToleranceDirection::LocalNewer => (
            format!(
                "{}: Local saves are newer by {}, within the {} safety window (use --force to back up immediately)",
                game_name, delta_str, tolerance_str
            ),
            format!(
                "{}: Local saves are newer by {} within the {} safety window (use --force to back up immediately)",
                game_name.yellow(),
                delta_str,
                tolerance_str
            ),
            "game.sync.within_tolerance.local_newer",
            "local_newer",
        ),
        ToleranceDirection::SnapshotNewer => (
            format!(
                "{}: Latest snapshot is newer by {}, within the {} safety window (use --force to restore immediately)",
                game_name, delta_str, tolerance_str
            ),
            format!(
                "{}: Latest snapshot is newer by {} within the {} safety window (use --force to restore immediately)",
                game_name.yellow(),
                delta_str,
                tolerance_str
            ),
            "game.sync.within_tolerance.snapshot_newer",
            "snapshot_newer",
        ),
    };

    emit_with_icon(
        Level::Info,
        code,
        char::from(NerdFont::Info),
        plain_msg,
        text_msg,
        Some(serde_json::json!({
            "game": game_name,
            "action": "within_tolerance",
            "direction": direction_value,
            "delta_seconds": delta_seconds,
            "tolerance_window_seconds": SYNC_TOLERANCE_SECONDS,
        })),
    );
}

pub fn report_restore_skipped(game_name: &str, snapshot_id: &str) {
    emit_with_icon(
        Level::Info,
        "game.sync.restore_skipped",
        char::from(NerdFont::Info),
        format!(
            "{}: Cloud checkpoint {} already matches your local saves (use --force to override)",
            game_name, snapshot_id
        ),
        format!(
            "{}: Cloud checkpoint {} already matches your local saves (use --force to override)",
            game_name.yellow(),
            snapshot_id
        ),
        Some(serde_json::json!({
            "game": game_name,
            "action": "restore_skipped",
            "snapshot_id": snapshot_id
        })),
    );
}

pub fn report_backup_skipped(game_name: &str, snapshot_id: &str) {
    emit_with_icon(
        Level::Info,
        "game.sync.backup_skipped",
        char::from(NerdFont::Info),
        format!(
            "{}: Cloud checkpoint {} already matches your local saves (use --force to override)",
            game_name, snapshot_id
        ),
        format!(
            "{}: Cloud checkpoint {} already matches your local saves (use --force to override)",
            game_name.yellow(),
            snapshot_id
        ),
        Some(serde_json::json!({
            "game": game_name,
            "action": "backup_skipped",
            "snapshot_id": snapshot_id
        })),
    );
}

pub fn report_backup_result(game_name: &str, result: &Result<(), anyhow::Error>) {
    match result {
        Ok(_) => {
            emit_with_icon(
                Level::Success,
                "game.sync.backup.completed",
                char::from(NerdFont::Check),
                format!("{}: Backup completed", game_name),
                format!("{}: Backup completed", game_name.green()),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "backup_completed"
                })),
            );
        }
        Err(e) => {
            emit_with_icon(
                Level::Error,
                "game.sync.backup.failed",
                char::from(NerdFont::CrossCircle),
                format!("{}: Backup failed: {}", game_name, e),
                format!("{}: Backup failed: {}", game_name.red(), e),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "backup_failed",
                    "error": e.to_string()
                })),
            );
        }
    }
}

pub fn report_restore_result(
    game_name: &str,
    snapshot_id: &str,
    result: &Result<(), anyhow::Error>,
) {
    match result {
        Ok(_) => {
            emit_with_icon(
                Level::Success,
                "game.sync.restore.completed",
                char::from(NerdFont::Check),
                format!("{}: Restore completed", game_name),
                format!("{}: Restore completed", game_name.green()),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "restore_completed",
                    "snapshot_id": snapshot_id
                })),
            );
        }
        Err(e) => {
            emit_with_icon(
                Level::Error,
                "game.sync.restore.failed",
                char::from(NerdFont::CrossCircle),
                format!("{}: Restore failed: {}", game_name, e),
                format!("{}: Restore failed: {}", game_name.red(), e),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "restore_failed",
                    "snapshot_id": snapshot_id,
                    "error": e.to_string()
                })),
            );
        }
    }
}

pub fn report_restore_latest_result(
    game_name: &str,
    snapshot_id: &str,
    result: &Result<(), anyhow::Error>,
) {
    match result {
        Ok(_) => {
            emit_with_icon(
                Level::Success,
                "game.sync.restore.latest.completed",
                char::from(NerdFont::Check),
                format!("{}: Restore completed", game_name),
                format!("{}: Restore completed", game_name.green()),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "restore_latest_completed",
                    "snapshot_id": snapshot_id
                })),
            );
        }
        Err(e) => {
            emit_with_icon(
                Level::Error,
                "game.sync.restore.latest.failed",
                char::from(NerdFont::CrossCircle),
                format!("{}: Restore failed: {}", game_name, e),
                format!("{}: Restore failed: {}", game_name.red(), e),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "restore_latest_failed",
                    "snapshot_id": snapshot_id,
                    "error": e.to_string()
                })),
            );
        }
    }
}

pub fn report_initial_backup_result(game_name: &str, result: &Result<(), anyhow::Error>) {
    match result {
        Ok(_) => {
            emit_with_icon(
                Level::Success,
                "game.sync.initial_backup.completed",
                char::from(NerdFont::Check),
                format!("{}: Initial backup completed", game_name),
                format!("{}: Initial backup completed", game_name.green()),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "initial_backup_completed"
                })),
            );
        }
        Err(e) => {
            emit_with_icon(
                Level::Error,
                "game.sync.initial_backup.failed",
                char::from(NerdFont::CrossCircle),
                format!("{}: Initial backup failed: {}", game_name, e),
                format!("{}: Initial backup failed: {}", game_name.red(), e),
                Some(serde_json::json!({
                    "game": game_name,
                    "action": "initial_backup_failed",
                    "error": e.to_string()
                })),
            );
        }
    }
}

pub fn report_error(game_name: &str, msg: &str) {
    emit_with_icon(
        Level::Error,
        "game.sync.error",
        char::from(NerdFont::CrossCircle),
        format!("{}: {}", game_name, msg),
        format!("{}: {}", game_name.red(), msg),
        Some(serde_json::json!({
            "game": game_name,
            "action": "error",
            "message": msg
        })),
    );
}

pub fn report_sync_failure(game_name: &str, e: &anyhow::Error) {
    emit_with_icon(
        Level::Error,
        "game.sync.failed",
        char::from(NerdFont::CrossCircle),
        format!("{}: Sync failed: {}", game_name, e),
        format!("{}: Sync failed: {}", game_name.red(), e),
        Some(serde_json::json!({
            "game": game_name,
            "action": "sync_failed",
            "error": e.to_string()
        })),
    );
}

pub fn report_installation_missing(name: &str) {
    emit_with_icon(
        Level::Error,
        "game.sync.installation_missing",
        char::from(NerdFont::CrossCircle),
        format!("Error: No installation found for game '{name}'."),
        format!("Error: No installation found for game '{}'.", name.red()),
        Some(serde_json::json!({
            "game": name,
            "action": "installation_missing"
        })),
    );
    emit_with_icon(
        Level::Info,
        "game.sync.hint.add",
        char::from(NerdFont::Info),
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
            "command": format!("{} game add", env!("CARGO_BIN_NAME")),
        })),
    );
}

pub fn report_no_games_configured() {
    emit_with_icon(
        Level::Warn,
        "game.sync.none",
        char::from(NerdFont::Warning),
        "No games configured for syncing.".to_string(),
        "No games configured for syncing.".to_string(),
        Some(serde_json::json!({
            "action": "no_games"
        })),
    );
    emit_with_icon(
        Level::Info,
        "game.sync.hint.add",
        char::from(NerdFont::Info),
        format!("Add games using '{} game add'.", env!("CARGO_BIN_NAME")),
        format!("Add games using '{} game add'.", env!("CARGO_BIN_NAME")),
        Some(serde_json::json!({
            "hint": "add_game",
            "command": format!("{} game add", env!("CARGO_BIN_NAME")),
        })),
    );
}

pub fn report_summary(total_synced: i32, total_skipped: i32, total_errors: i32) {
    emit_separator();
    let summary_data = serde_json::json!({
        "synced": total_synced,
        "skipped": total_skipped,
        "errors": total_errors
    });

    let summary_title = if matches!(get_output_format(), OutputFormat::Json) {
        "Sync Summary".to_string()
    } else {
        format!(
            "{} {} Sync Summary",
            char::from(NerdFont::Chart),
            char::from(NerdFont::List)
        )
    };

    if matches!(get_output_format(), OutputFormat::Json) {
        emit(Level::Info, "game.sync.summary.title", &summary_title, None);
        let summary_text = format!(
            "  Synced: {}\n  Skipped: {}\n  Errors: {}",
            total_synced, total_skipped, total_errors
        );
        emit(
            Level::Info,
            "game.sync.summary",
            &summary_text,
            Some(summary_data),
        );
        emit_separator();
    } else {
        emit(
            Level::Info,
            "game.sync.summary.title",
            &summary_title,
            Some(summary_data),
        );

        let entries = [
            (
                Level::Success,
                Some(char::from(NerdFont::Check)),
                "Synced",
                total_synced,
                "game.sync.summary.synced",
            ),
            (
                Level::Info,
                Some(char::from(NerdFont::Flag)),
                "Skipped",
                total_skipped,
                "game.sync.summary.skipped",
            ),
            (
                Level::Error,
                Some(char::from(NerdFont::CrossCircle)),
                "Errors",
                total_errors,
                "game.sync.summary.errors",
            ),
        ];

        let label_width = entries
            .iter()
            .map(|(_, _, label, _, _)| label.len())
            .max()
            .unwrap_or(0);
        let column_width = label_width + 4;

        for (level, icon, label, value, code) in entries {
            let label_with_icon = if matches!(get_output_format(), OutputFormat::Json) {
                format!("{label}:")
            } else {
                match icon {
                    Some(icon) => format!("{icon} {label}:"),
                    None => format!("  {label}:"),
                }
            };
            let padded_label = format!("{label_with_icon:<width$}", width = column_width);
            let message = format!("{padded_label} {value}");
            emit(
                level,
                code,
                &message,
                Some(serde_json::json!({
                    "label": label.to_lowercase(),
                    "count": value
                })),
            );
        }

        emit_separator();
    }
}
