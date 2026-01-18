//! Control flow helpers for alternative menus.

use anyhow::Result;

use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;

/// Explicit control flow for menu operations.
/// This replaces confusing `Result<bool>` patterns.
#[derive(Clone, Copy)]
pub(crate) enum Flow {
    /// Continue showing the current menu (refresh and loop)
    Continue,
    /// Action completed successfully, exit current menu
    Done,
    /// User cancelled, exit current menu
    Cancelled,
}

/// Show a message and return the appropriate flow
pub(crate) fn message_and_continue(msg: &str) -> Result<Flow> {
    FzfWrapper::message(msg)?;
    Ok(Flow::Continue)
}

pub(crate) fn message_and_done(msg: &str) -> Result<Flow> {
    FzfWrapper::message(msg)?;
    Ok(Flow::Done)
}

pub(crate) fn emit_cancelled() {
    emit(
        Level::Info,
        "dot.alternative.cancelled",
        &format!("{} Selection cancelled", char::from(NerdFont::Info)),
        None,
    );
}
