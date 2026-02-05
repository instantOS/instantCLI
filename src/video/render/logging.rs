use crate::ui::prelude::{emit, Level};

pub(super) fn log_event(level: Level, code: &str, message: impl Into<String>) {
    let message = message.into();
    emit(level, code, &message, None);
}
