use anyhow::Result;
use colored::Colorize;
use serde_json::{Value, json};

use crate::common::progress::create_spinner;
use crate::ui::nerd_font::NerdFont;
use crate::ui::{Level, OutputFormat, emit, get_output_format};

use super::SyncReport;

pub(super) fn stage<T>(message: String, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    if get_output_format() == OutputFormat::Json {
        emit(Level::Info, "game.sync.progress", &message, None);
        return operation();
    }
    let spinner = create_spinner(message);
    let result = operation();
    spinner.finish_and_clear();
    result
}

pub(super) fn report(level: Level, code: &str, message: &str, data: Value) {
    let icon = match level {
        Level::Success => NerdFont::Check,
        Level::Error => NerdFont::CrossCircle,
        Level::Warn => NerdFont::Warning,
        _ => NerdFont::Info,
    };
    report_with_icon(level, code, icon, message, data);
}

pub(super) fn report_with_icon(
    level: Level,
    code: &str,
    icon: NerdFont,
    message: &str,
    data: Value,
) {
    let message = if get_output_format() == OutputFormat::Text {
        format!("{icon} {message}")
    } else {
        message.to_owned()
    };
    emit(level, code, &message, Some(data));
}

pub(super) fn already_in_sync(name: &str) {
    let message = format!("{name}: Already in sync");
    let message = if get_output_format() == OutputFormat::Text {
        message.dimmed().to_string()
    } else {
        message
    };
    let level = match get_output_format() {
        OutputFormat::Text => Level::Info,
        OutputFormat::Json => Level::Success,
    };
    report_with_icon(
        level,
        "game.sync.already_in_sync",
        NerdFont::Check,
        &message,
        json!({"game": name, "action": "already_in_sync"}),
    );
}

pub(super) fn no_games_configured() {
    report(
        Level::Info,
        "game.sync.none",
        "No save owners configured for syncing.",
        json!({"action": "no_games"}),
    );
    report(
        Level::Info,
        "game.sync.hint.add",
        &format!("Add games using '{} game add'.", env!("CARGO_BIN_NAME")),
        json!({"hint": "add_game", "command": format!("{} game add", env!("CARGO_BIN_NAME"))}),
    );
}

pub(super) fn summary(report: &SyncReport) {
    let message = match get_output_format() {
        OutputFormat::Json => report.completion_message(),
        OutputFormat::Text => summary_text(&report.summary),
    };
    emit(
        Level::Info,
        "game.sync.summary",
        &message,
        Some(json!({
            "backed_up": report.summary.backed_up,
            "restored": report.summary.restored,
            "skipped": report.summary.skipped,
            "errors": report.summary.errors,
        })),
    );
}

fn summary_text(summary: &super::SyncSummary) -> String {
    let separator = "─".repeat(80);
    let mut lines = vec![
        separator.clone(),
        format!("{} Sync Summary", NerdFont::Chart),
    ];
    for (icon, label, count, is_error) in [
        (NerdFont::Upload, "Backed up", summary.backed_up, false),
        (NerdFont::Download, "Restored", summary.restored, false),
        (NerdFont::Flag, "Skipped", summary.skipped, false),
        (NerdFont::CrossCircle, "Errors", summary.errors, true),
    ] {
        let label = format!("{label}:");
        let line = format!("  {icon} {label:<10} {count}");
        let styled = if count == 0 {
            line.dimmed()
        } else if is_error {
            line.red().bold()
        } else if label == "Skipped:" {
            line.normal()
        } else {
            line.green().bold()
        };
        lines.push(styled.to_string());
    }
    lines.push(separator);
    lines.join("\n")
}
