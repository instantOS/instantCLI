//! Notification center UI
//!
//! Main entry point for the interactive notification browser.
//! Mirrors the `ins settings` UI pattern: FZF-based menus with
//! `FzfSelectable` items, `MenuCursor` for position tracking, and
//! `PreviewBuilder` for rich previews.

use anyhow::{Context, Result};

use crate::menu_utils::{FzfSelectable, MenuCursor, select_one_with_style_at};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

use super::db::NotifyDb;
use super::handlers;
use super::items::{
    NotificationDetailAction, NotificationDetailItem, NotificationDetailPreview,
    NotificationListItem, NotifyMainItem,
};

/// Run the interactive notification center UI.
pub fn run_notify_ui(debug: bool, mut daemon_running: bool) -> Result<()> {
    let db = NotifyDb::open().context("opening notification database")?;

    let mut cursor = MenuCursor::new();

    loop {
        match run_main_menu(&db, daemon_running, debug, &mut cursor)? {
            MenuAction::OpenNotification { id, main_cursor } => {
                cursor = main_cursor;
                handle_notification_detail(&db, id, debug)?;
            }
            MenuAction::OpenOptions { main_cursor } => {
                cursor = main_cursor;
                super::options::run_options_menu(&db, debug)?;
            }
            MenuAction::EnableCapture { main_cursor } => {
                cursor = main_cursor;
                super::service::enable_and_start()?;
                daemon_running = true;
            }
            MenuAction::Exit => break,
        }
    }

    Ok(())
}

enum MenuAction {
    OpenNotification { id: i64, main_cursor: MenuCursor },
    OpenOptions { main_cursor: MenuCursor },
    EnableCapture { main_cursor: MenuCursor },
    Exit,
}

/// Build the main menu items from the database.
fn build_main_items(db: &NotifyDb, daemon_running: bool) -> Result<Vec<NotifyMainItem>> {
    let notifications = db.list()?;
    let unread = db.unread_count()?;

    let mut items: Vec<NotifyMainItem> = Vec::new();

    // Unread count header (if any unread)
    if unread > 0 {
        items.push(NotifyMainItem::UnreadCount(unread));
    }

    if !daemon_running {
        items.push(NotifyMainItem::EnableCapture);
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
    daemon_running: bool,
    _debug: bool,
    cursor: &mut MenuCursor,
) -> Result<MenuAction> {
    let items = build_main_items(db, daemon_running)?;

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
        Some(NotifyMainItem::EnableCapture) => {
            cursor.update(&NotifyMainItem::EnableCapture, &items);
            MenuAction::EnableCapture {
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
/// Shows the notification content and actions (back, mark unread, and delete).
fn handle_notification_detail(db: &NotifyDb, id: i64, _debug: bool) -> Result<()> {
    let Some(mut notification) = db.get(id)? else {
        emit(
            Level::Warn,
            "notify.not_found",
            &format!("{} Notification not found.", char::from(NerdFont::Warning)),
            None,
        );
        return Ok(());
    };

    // Mark as read when viewed
    db.mark_read(id)?;
    notification.read = true;

    let items = build_detail_items(&notification);
    let initial_index = items.iter().position(FzfSelectable::fzf_is_selectable);
    let selection = select_one_with_style_at(items.clone(), initial_index)?;

    match selection {
        Some(NotificationDetailItem::Action { action, .. }) => match action {
            NotificationDetailAction::Back => Ok(()),
            NotificationDetailAction::MarkUnread => {
                db.mark_unread(id)?;
                emit(
                    Level::Success,
                    "notify.marked_unread",
                    &format!("{} Marked as unread.", char::from(NerdFont::Check)),
                    None,
                );
                Ok(())
            }
            NotificationDetailAction::Delete => {
                handlers::handle_delete(db, id)?;
                Ok(())
            }
        },
        _ => Ok(()),
    }
}

/// Build the detail menu items for a notification.
fn build_detail_items(notif: &super::db::Notification) -> Vec<NotificationDetailItem> {
    let mut items = Vec::new();

    let action_labels = if notif.actions.is_empty() {
        None
    } else {
        let labels = notif
            .actions
            .iter()
            .map(|action| action.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let state = if notif.active { "live" } else { "expired" };
        Some(format!("{labels} ({state})"))
    };

    // Info items (non-selectable)
    items.push(NotificationDetailItem::Title(notif.title.clone()));
    items.push(NotificationDetailItem::App(notif.app_name.clone()));
    items.push(NotificationDetailItem::Time(notif.timestamp.clone()));
    if let Some(labels) = &action_labels {
        items.push(NotificationDetailItem::Actions(labels.clone()));
    }

    // Body preview (truncated for display)
    let body_preview: String = notif.body.chars().take(500).collect();
    items.push(NotificationDetailItem::Body(body_preview));

    // Separator
    items.push(NotificationDetailItem::Separator);

    let preview = NotificationDetailPreview {
        title: notif.title.clone(),
        app_name: notif.app_name.clone(),
        timestamp: notif.timestamp.clone(),
        body: notif.body.clone(),
        actions: action_labels,
    };

    // Actions
    items.push(NotificationDetailItem::Action {
        action: NotificationDetailAction::Back,
        preview: preview.clone(),
    });
    if notif.read {
        items.push(NotificationDetailItem::Action {
            action: NotificationDetailAction::MarkUnread,
            preview: preview.clone(),
        });
    }
    items.push(NotificationDetailItem::Action {
        action: NotificationDetailAction::Delete,
        preview,
    });

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::db::{Notification, NotificationAction};

    fn notification() -> Notification {
        Notification {
            id: 42,
            timestamp: "2026-07-22 12:34".to_string(),
            app_name: "Bluetooth".to_string(),
            title: "Pair device?".to_string(),
            body: "Allow the keyboard to pair.".to_string(),
            read: true,
            actions: vec![NotificationAction {
                key: "pair".to_string(),
                label: "Pair".to_string(),
            }],
            active: true,
            invoked_action: None,
        }
    }

    #[test]
    fn detail_menu_shows_title_and_only_detail_actions() {
        let items = build_detail_items(&notification());

        assert!(matches!(
            items.first(),
            Some(NotificationDetailItem::Title(title)) if title == "Pair device?"
        ));

        let actions = items
            .iter()
            .filter_map(|item| match item {
                NotificationDetailItem::Action { action, .. } => Some(action),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            vec![
                &NotificationDetailAction::Back,
                &NotificationDetailAction::MarkUnread,
                &NotificationDetailAction::Delete,
            ]
        );
    }

    #[test]
    fn first_selectable_detail_item_is_back_with_full_content() {
        let items = build_detail_items(&notification());
        let first_selectable = items
            .iter()
            .find(|item| item.fzf_is_selectable())
            .expect("detail menu should contain an action");

        let NotificationDetailItem::Action { action, preview } = first_selectable else {
            panic!("first selectable item should be an action");
        };
        assert_eq!(action, &NotificationDetailAction::Back);
        assert_eq!(preview.title, "Pair device?");
        assert_eq!(preview.body, "Allow the keyboard to pair.");
        assert_eq!(preview.actions.as_deref(), Some("Pair (live)"));
    }
}
