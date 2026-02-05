use crate::ui::prelude::{Level, emit};

pub(super) fn log_event(level: Level, code: &str, message: impl Into<String>) {
    let message = message.into();
    emit(level, code, &message, None);
}
