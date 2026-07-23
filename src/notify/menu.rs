//! Notification center UI
//!
//! Main entry point for the interactive notification browser.
//! Mirrors the `ins settings` UI pattern: FZF-based menus with
//! `FzfSelectable` items, `MenuCursor` for position tracking, and
//! `PreviewBuilder` for rich previews.

use anyhow::{Context, Result};

use crate::menu_utils::{
    HeaderBuilder, MenuCursor, select_one_with_style_at, select_one_with_style_at_header,
};
use crate::ui::catppuccin::colors;
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
///
/// Options are placed at the top so they are always immediately accessible;
/// individual notifications follow below.
fn build_main_items(db: &NotifyDb, daemon_running: bool) -> Result<(Vec<NotifyMainItem>, i64)> {
    let notifications = db.list()?;
    let unread = db.unread_count()?;

    let mut items: Vec<NotifyMainItem> = Vec::new();

    // Options at the top
    items.push(NotifyMainItem::Close);
    items.push(NotifyMainItem::Options);
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

    Ok((items, unread))
}

fn run_main_menu(
    db: &NotifyDb,
    daemon_running: bool,
    _debug: bool,
    cursor: &mut MenuCursor,
) -> Result<MenuAction> {
    let (items, unread) = build_main_items(db, daemon_running)?;

    if items.is_empty() {
        emit(
            Level::Info,
            "notify.empty",
            &format!("{} No notifications.", char::from(NerdFont::Bell)),
            None,
        );
        return Ok(MenuAction::Exit);
    }

    // When the menu is opened fresh (no prior cursor state), default to the
    // first notification — the options at the top are always visible and
    // immediately accessible via a single up-arrow press.
    let initial_index = cursor
        .initial_index(&items)
        .or_else(|| items.iter().position(|item| matches!(item, NotifyMainItem::Notification(_))));
    let count_color = if unread > 0 {
        colors::YELLOW
    } else {
        colors::SUBTEXT0
    };
    let header = HeaderBuilder::new(NerdFont::Bell, "Notification Center")
        .status(
            NerdFont::EnvelopeOpen,
            format!("{unread} unread"),
            count_color,
        )
        .build();
    let selection = select_one_with_style_at_header(items.clone(), initial_index, header)?;

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
        None => MenuAction::Exit,
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
    let selection = select_one_with_style_at(items, Some(0))?;

    match selection {
        Some(item) => match item.action {
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

    let preview = NotificationDetailPreview {
        title: notif.title.clone(),
        app_name: notif.app_name.clone(),
        timestamp: notif.timestamp.clone(),
        body: notif.body.clone(),
        actions: action_labels,
    };

    let mut items = vec![NotificationDetailItem {
        action: NotificationDetailAction::Back,
        preview: preview.clone(),
    }];
    if notif.read {
        items.push(NotificationDetailItem {
            action: NotificationDetailAction::MarkUnread,
            preview: preview.clone(),
        });
    }
    items.push(NotificationDetailItem {
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
    fn detail_menu_contains_only_actions_with_notification_content() {
        let items = build_detail_items(&notification());

        let actions = items.iter().map(|item| &item.action).collect::<Vec<_>>();
        assert_eq!(
            actions,
            vec![
                &NotificationDetailAction::Back,
                &NotificationDetailAction::MarkUnread,
                &NotificationDetailAction::Delete,
            ]
        );
        assert!(
            items
                .iter()
                .all(|item| item.preview.title == "Pair device?")
        );
        assert!(
            items
                .iter()
                .all(|item| item.preview.body == "Allow the keyboard to pair.")
        );
    }

    #[test]
    fn first_selectable_detail_item_is_back_with_full_content() {
        let items = build_detail_items(&notification());
        let first = items.first().expect("detail menu should contain an action");

        assert_eq!(first.action, NotificationDetailAction::Back);
        assert_eq!(first.preview.title, "Pair device?");
        assert_eq!(first.preview.body, "Allow the keyboard to pair.");
        assert_eq!(first.preview.actions.as_deref(), Some("Pair (live)"));
    }
}
