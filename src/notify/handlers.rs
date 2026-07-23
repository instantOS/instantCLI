//! Handlers for notification actions
//!
//! Called from the menu when a user selects a notification action.

use anyhow::Result;

use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

use super::db::NotifyDb;

/// Handle deleting a notification by ID.
pub fn handle_delete(db: &NotifyDb, id: i64) -> Result<()> {
    anyhow::ensure!(db.delete(id)?, "notification {id} was not found");
    emit(
        Level::Success,
        "notify.deleted",
        &format!("{} Deleted notification {id}.", char::from(NerdFont::Check)),
        None,
    );
    Ok(())
}
