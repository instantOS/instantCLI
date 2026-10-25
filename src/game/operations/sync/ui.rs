use anyhow::Result;
use serde_json::{Value, json};

use crate::common::progress::create_spinner;
use crate::ui::{Level, OutputFormat, emit, get_output_format};

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
    let message = if get_output_format() == OutputFormat::Text {
        let icon = match level {
            Level::Success => crate::ui::nerd_font::NerdFont::Check,
            Level::Error => crate::ui::nerd_font::NerdFont::CrossCircle,
            _ => crate::ui::nerd_font::NerdFont::Info,
        };
        format!("{icon} {message}")
    } else {
        message.to_owned()
    };
    emit(level, code, &message, Some(data));
}

pub(super) fn already_in_sync(name: &str) {
    report(
        Level::Success,
        "game.sync.already_in_sync",
        &format!("{name}: Already in sync"),
        json!({"game": name, "action": "already_in_sync"}),
    );
}
