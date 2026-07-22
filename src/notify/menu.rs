//! Notification center UI
//!
//! Main entry point for the interactive notification browser.
//! Mirrors the `ins settings` UI pattern: FZF-based menus with
//! `FzfSelectable` items, `MenuCursor` for position tracking, and
//! `PreviewBuilder` for rich previews.

use anyhow::{Context, Result};

use crate::menu_utils::{MenuCursor, select_one_with_style_at};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

use super::db::NotifyDb;
use super::handlers;
use super::items::{NotifyMainItem, NotificationListItem, NotificationDetailItem, NotificationDetailAction};

/// Run the interactive notification center UI.
pub fn run_notify_ui(debug: bool) -> Result<()> {
    let db = NotifyDb::open().context("opening notification database")?;

    let mut cursor = MenuCursor::new();

    loop {
        match run_main_menu(&db, debug, &mut cursor)? {
            MenuAction::OpenNotification { id, main_cursor } => {
                cursor = main_cursor;
                if !handle_notification_detail(&db, id, debug)? {
                    break;
                }
            }
            MenuAction::OpenOptions { main_cursor } => {
                cursor = main_cursor;
                super::options::run_options_menu(&db, debug)?;
            }
            MenuAction::Exit => break,
        }
    }

    Ok(())
}

enum MenuAction {
    OpenNotification { id: i64, main_cursor: MenuCursor },
    OpenOptions { main_cursor: MenuCursor },
    Exit,
}

/// Build the main menu items from the database.
fn build_main_items(db: &NotifyDb) -> Result<Vec<NotifyMainItem>> {
    let notifications = db.list()?;
    let unread = db.unread_count()?;

    let mut items: Vec<NotifyMainItem> = Vec::new();

    // Unread count header (if any unread)
    if unread > 0 {
        items.push(NotifyMainItem::UnreadCount(unread));
    }

    // Individual notifications
    for n in notifications {
        items.push(NotifyMainItem::Notification(NotificationListItem {
            id: n.id,
            app_name: n.app_name,
            title: n.title,
            body: n.body,
            timestamp: n.timestamp,
            read: n.read,
        }));
    }

    // Options and close at the bottom
    items.push(NotifyMainItem::Options);
    items.push(NotifyMainItem::Close);

    Ok(items)
}

fn run_main_menu(
    db: &NotifyDb,
    _debug: bool,
    cursor: &mut MenuCursor,
) -> Result<MenuAction> {
    let items = build_main_items(db)?;

    if items.is_empty() {
        emit(
            Level::Info,
            "notify.empty",
            &format!("{} No notifications.", char::from(NerdFont::Bell)),
            None,
        );
        return Ok(MenuAction::Exit);
    }

    let initial_index = cursor.initial_index(&items);
    let selection = select_one_with_style_at(items.clone(), initial_index)?;

    let action = match selection {
        Some(NotifyMainItem::Notification(n)) => {
            cursor.update(&NotifyMainItem::Notification(n.clone()), &items);
            MenuAction::OpenNotification {
                id: n.id,
                main_cursor: cursor.clone(),
            }
        }
        Some(NotifyMainItem::Options) => {
            cursor.update(&NotifyMainItem::Options, &items);
            MenuAction::OpenOptions {
                main_cursor: cursor.clone(),
            }
        }
        Some(NotifyMainItem::UnreadCount(_)) | None => MenuAction::Exit,
        Some(NotifyMainItem::Close) => MenuAction::Exit,
    };

    Ok(action)
}

/// Handle the detail view for a single notification.
///
/// Shows the notification content and actions (mark read/unread, delete, close).
/// Returns `false` if the user wants to exit the notification center entirely.
fn handle_notification_detail(db: &NotifyDb, id: i64, _debug: bool) -> Result<bool> {
    // Fetch the notification
    let notifications = db.list()?;
    let Some(notif) = notifications.iter().find(|n| n.id == id) else {
        emit(
            Level::Warn,
            "notify.not_found",
            &format!("{} Notification not found.", char::from(NerdFont::Warning)),
            None,
        );
        return Ok(true);
    };

    // Mark as read when viewed
    db.mark_read(id)?;

    let mut cursor = MenuCursor::new();

    loop {
        // Build detail items
        let items = build_detail_items(notif);

        let initial_index = cursor.initial_index(&items);
        let selection = select_one_with_style_at(items.clone(), initial_index)?;

        match selection {
            Some(NotificationDetailItem::Action(action)) => {
                cursor.update(&NotificationDetailItem::Action(action.clone()), &items);

                match action {
                    NotificationDetailAction::Back => return Ok(true),
                    NotificationDetailAction::MarkUnread => {
                        db.mark_unread(id)?;
                        emit(
                            Level::Success,
                            "notify.marked_unread",
                            &format!("{} Marked as unread.", char::from(NerdFont::Check)),
                            None,
                        );
                        return Ok(true);
                    }
                    NotificationDetailAction::Delete => {
                        handlers::handle_delete(db, id)?;
                        return Ok(true);
                    }
                    NotificationDetailAction::Close => return Ok(false),
                }
            }
            _ => return Ok(true),
        }
    }
}

/// Build the detail menu items for a notification.
fn build_detail_items(notif: &super::db::Notification) -> Vec<NotificationDetailItem> {
    let mut items = Vec::new();

    // Info items (non-selectable)
    items.push(NotificationDetailItem::App(notif.app_name.clone()));
    items.push(NotificationDetailItem::Time(notif.timestamp.clone()));

    // Body preview (truncated for display)
    let body_preview: String = notif.body.chars().take(500).collect();
    items.push(NotificationDetailItem::Body(body_preview));

    // Separator
    items.push(NotificationDetailItem::Separator);

    // Actions
    items.push(NotificationDetailItem::Action(NotificationDetailAction::Back));
    if notif.read {
        items.push(NotificationDetailItem::Action(NotificationDetailAction::MarkUnread));
    }
    items.push(NotificationDetailItem::Action(NotificationDetailAction::Delete));
    items.push(NotificationDetailItem::Action(NotificationDetailAction::Close));

    items
}
