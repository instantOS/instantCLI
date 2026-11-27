//! Notification center UI
//!
//! Main entry point for the interactive notification browser.
//! Mirrors the `ins settings` UI pattern: FZF-based menus with
//! `FzfSelectable` items, `MenuCursor` for position tracking, and
//! `PreviewBuilder` for rich previews.

use anyhow::{Context, Result};

use crate::menu_utils::{
    FzfWrapper, HeaderBuilder, MenuCursor, MenuKey, MenuKeybind, MenuSort, choose_sort,
    sort_keybind,
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
use super::sort::{NotifySort, sort_notifications};

/// Run the interactive notification center UI.
pub fn run_notify_ui(debug: bool, mut daemon_running: bool) -> Result<()> {
    let db = NotifyDb::open().context("opening notification database")?;

    let mut cursor = MenuCursor::new();
    let mut sort = NotifySort::default();

    loop {
        match run_main_menu(&db, daemon_running, &mut cursor, &mut sort)? {
            MenuAction::OpenNotification { id, main_cursor } => {
                cursor = main_cursor;
                handle_notification_detail(&db, id)?;
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
            MenuAction::DeleteNotification { id, main_cursor } => {
                cursor = main_cursor;
                handlers::handle_delete(&db, id)?;
            }
            MenuAction::ToggleRead {
                id,
                read,
                main_cursor,
            } => {
                cursor = main_cursor;
                if read {
                    db.mark_unread(id)?;
                    emit(
                        Level::Success,
                        "notify.marked_unread",
                        &format!("{} Marked as unread.", char::from(NerdFont::Check)),
                        None,
                    );
                } else {
                    db.mark_read(id)?;
                    emit(
                        Level::Success,
                        "notify.marked_read",
                        &format!("{} Marked as read.", char::from(NerdFont::Check)),
                        None,
                    );
                }
            }
            MenuAction::Refresh { main_cursor } => cursor = main_cursor,
            MenuAction::Exit => break,
        }
    }

    Ok(())
}

enum MenuAction {
    OpenNotification {
        id: i64,
        main_cursor: MenuCursor,
    },
    OpenOptions {
        main_cursor: MenuCursor,
    },
    EnableCapture {
        main_cursor: MenuCursor,
    },
    DeleteNotification {
        id: i64,
        main_cursor: MenuCursor,
    },
    ToggleRead {
        id: i64,
        read: bool,
        main_cursor: MenuCursor,
    },
    Refresh {
        main_cursor: MenuCursor,
    },
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotifyKeybindAction {
    Delete,
    ToggleRead,
    Sort,
    Options,
    EnableCapture,
}

fn notification_keybinds(daemon_running: bool) -> Result<Vec<MenuKeybind<NotifyKeybindAction>>> {
    let mut keybinds = vec![
        MenuKeybind::new(
            MenuKey::new("ctrl-d")?,
            "delete",
            NotifyKeybindAction::Delete,
        ),
        MenuKeybind::new(
            MenuKey::new("ctrl-u")?,
            "toggle read",
            NotifyKeybindAction::ToggleRead,
        ),
        sort_keybind()?.map_action(NotifyKeybindAction::Sort),
        MenuKeybind::new(
            MenuKey::new("ctrl-o")?,
            "options",
            NotifyKeybindAction::Options,
        ),
    ];
    if !daemon_running {
        keybinds.push(MenuKeybind::new(
            MenuKey::new("ctrl-e")?,
            "enable capture",
            NotifyKeybindAction::EnableCapture,
        ));
    }
    Ok(keybinds)
}

/// Build the main menu items from the database.
///
/// Options are placed at the top so they are always immediately accessible;
/// individual notifications follow below in `sort` order.
fn build_main_items(
    db: &NotifyDb,
    daemon_running: bool,
    sort: NotifySort,
) -> Result<(Vec<NotifyMainItem>, i64)> {
    let notifications = sort_notifications(
        db.list()?
            .into_iter()
            .map(NotificationListItem::from)
            .collect(),
        sort,
    );
    let unread = db.unread_count()?;

    let mut items: Vec<NotifyMainItem> = Vec::new();

    // Options at the top
    items.push(NotifyMainItem::Close);
    items.push(NotifyMainItem::Options);
    if !daemon_running {
        items.push(NotifyMainItem::EnableCapture);
    }

    // Individual notifications
    items.extend(notifications.into_iter().map(NotifyMainItem::Notification));

    Ok((items, unread))
}

/// Show the main menu until an entry is chosen or the menu is dismissed.
///
/// Pressing the sort key chooses a new order for the notifications and
/// reopens the menu with the entry that was focused still under the cursor.
fn run_main_menu(
    db: &NotifyDb,
    daemon_running: bool,
    cursor: &mut MenuCursor,
    sort: &mut NotifySort,
) -> Result<MenuAction> {
    let keybinds = notification_keybinds(daemon_running)?;
    loop {
        let (items, unread) = build_main_items(db, daemon_running, *sort)?;

        // When the menu is opened fresh (no prior cursor state), default to the
        // first notification — the options at the top are always visible and
        // immediately accessible via a single up-arrow press.
        let initial_index = cursor.initial_index(&items).or_else(|| {
            items
                .iter()
                .position(|item| matches!(item, NotifyMainItem::Notification(_)))
        });
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
            .field("Sort", sort.label())
            .build();
        let selection = FzfWrapper::menu()
            .cursor(initial_index)
            .header(header)
            .items(items.clone())
            .keybinds(&keybinds)
            .select()?;

        let action = match selection {
            crate::menu_utils::DialogOutcome::Submitted(selection) => {
                let selected_item = selection.items.into_iter().next();
                if let Some(item) = &selected_item {
                    cursor.update(item, &items);
                }
                let main_cursor = cursor.clone();
                match (selection.action, selected_item) {
                    // Reordering the notifications must not move the cursor
                    // off the one the user was on.
                    (Some(NotifyKeybindAction::Sort), _) => {
                        if let Some(chosen) = choose_sort(*sort)? {
                            *sort = chosen;
                        }
                        continue;
                    }
                    (Some(NotifyKeybindAction::Delete), Some(NotifyMainItem::Notification(n))) => {
                        MenuAction::DeleteNotification {
                            id: n.id,
                            main_cursor,
                        }
                    }
                    (
                        Some(NotifyKeybindAction::ToggleRead),
                        Some(NotifyMainItem::Notification(n)),
                    ) => MenuAction::ToggleRead {
                        id: n.id,
                        read: n.read,
                        main_cursor,
                    },
                    (Some(NotifyKeybindAction::Options), _) => {
                        MenuAction::OpenOptions { main_cursor }
                    }
                    (Some(NotifyKeybindAction::EnableCapture), _) => {
                        MenuAction::EnableCapture { main_cursor }
                    }
                    (Some(NotifyKeybindAction::Delete | NotifyKeybindAction::ToggleRead), _)
                    | (None, None) => MenuAction::Refresh { main_cursor },
                    (None, Some(NotifyMainItem::Notification(n))) => MenuAction::OpenNotification {
                        id: n.id,
                        main_cursor,
                    },
                    (None, Some(NotifyMainItem::Options)) => {
                        MenuAction::OpenOptions { main_cursor }
                    }
                    (None, Some(NotifyMainItem::EnableCapture)) => {
                        MenuAction::EnableCapture { main_cursor }
                    }
                    (None, Some(NotifyMainItem::Close)) => MenuAction::Exit,
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => MenuAction::Exit,
        };

        return Ok(action);
    }
}

/// Handle the detail view for a single notification.
///
/// Shows the notification content and actions (back, mark unread, and delete).
fn handle_notification_detail(db: &NotifyDb, id: i64) -> Result<()> {
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
    let selection = FzfWrapper::menu()
        .initial_index(0)
        .items(items)
        .padded()
        .select_one()?;

    match selection {
        crate::menu_utils::DialogOutcome::Submitted(item) => match item.action {
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
        crate::menu_utils::DialogOutcome::Cancelled => Ok(()),
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
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};
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

    #[test]
    fn capture_keybind_only_appears_while_daemon_is_stopped() {
        let stopped = notification_keybinds(false).unwrap();
        let running = notification_keybinds(true).unwrap();

        assert!(stopped.iter().any(|bind| bind.key.as_str() == "ctrl-e"));
        assert!(!running.iter().any(|bind| bind.key.as_str() == "ctrl-e"));
        for key in ["ctrl-d", "ctrl-u", "ctrl-s", "ctrl-o"] {
            assert!(running.iter().any(|bind| bind.key.as_str() == key));
        }
        let sort = running
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-s")
            .unwrap();
        assert_eq!(sort.label, "sort");
        assert_eq!(sort.action, NotifyKeybindAction::Sort);
    }

    /// Notifications in arrival order. The database lists them newest first:
    /// Spotify, Discord, Discord, Slack. Spotify and the earlier Discord
    /// message were read.
    fn populated_db() -> Result<(tempfile::TempDir, NotifyDb)> {
        let dir = tempfile::tempdir()?;
        let db = NotifyDb::open_at(&dir.path().join("notifications.db"))?;
        db.add("2026-11-24 09:00", "Slack", "Build finished", "main passed")?;
        let read_discord = db.add(
            "2026-11-24 09:05",
            "Discord",
            "Second message",
            "anyone there?",
        )?;
        db.add("2026-11-24 09:10", "Discord", "First message", "hello")?;
        let spotify = db.add(
            "2026-11-24 09:15",
            "Spotify",
            "Now playing",
            "Song - Artist",
        )?;
        db.mark_read(read_discord)?;
        db.mark_read(spotify)?;
        Ok((dir, db))
    }

    fn notification_ids(db: &NotifyDb, sort: NotifySort) -> Result<Vec<i64>> {
        Ok(build_main_items(db, false, sort)?
            .0
            .into_iter()
            .filter_map(|item| match item {
                NotifyMainItem::Notification(notification) => Some(notification.id),
                _ => None,
            })
            .collect())
    }

    #[test]
    fn notifications_default_to_the_newest_first() -> Result<()> {
        let (_dir, db) = populated_db()?;

        assert_eq!(notification_ids(&db, NotifySort::default())?, [4, 3, 2, 1]);
        assert_eq!(
            notification_ids(&db, NotifySort::Application)?,
            [3, 2, 1, 4]
        );
        Ok(())
    }

    #[test]
    fn empty_database_still_offers_the_fixed_actions() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let db = NotifyDb::open_at(&dir.path().join("notifications.db"))?;

        let (items, unread) = build_main_items(&db, false, NotifySort::default())?;

        assert_eq!(unread, 0);
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0], NotifyMainItem::Close));
        assert!(matches!(items[1], NotifyMainItem::Options));
        assert!(matches!(items[2], NotifyMainItem::EnableCapture));
        Ok(())
    }

    #[test]
    fn menu_keeps_its_fixed_entries_above_the_notifications() -> Result<()> {
        let (_dir, db) = populated_db()?;

        let (items, unread) = build_main_items(&db, false, NotifySort::Unread)?;

        assert_eq!(unread, 2);
        assert!(matches!(items[0], NotifyMainItem::Close));
        assert!(matches!(items[1], NotifyMainItem::Options));
        assert!(matches!(items[2], NotifyMainItem::EnableCapture));
        assert_eq!(items.len(), 7);
        Ok(())
    }

    #[test]
    fn sort_key_reorders_notifications_and_keeps_the_focused_one() -> Result<()> {
        let (_dir, db) = populated_db()?;
        let mut cursor = MenuCursor::new();
        let mut sort = NotifySort::default();
        // Rows: three fixed entries, then Spotify, Discord, Discord, Slack.
        // Spotify is focused when the sort key is pressed; Unread first is the
        // fourth row of the sort chooser, after Back.
        let _mock = MockQueue::new()
            .keybind_action("ctrl-s", vec![3])
            .select_index(3)
            .select_index(5)
            .guard();

        let action = run_main_menu(&db, false, &mut cursor, &mut sort)?;

        assert!(matches!(action, MenuAction::OpenNotification { id: 4, .. }));
        assert_eq!(sort, NotifySort::Unread);
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn focused_notification_follows_the_new_order() -> Result<()> {
        let (_dir, db) = populated_db()?;
        let mut cursor = MenuCursor::new();
        let mut sort = NotifySort::default();
        // By application is the fifth row of the sort chooser, after Back.
        // It groups Discord above Slack, so Slack moves from row 6 to row 5.
        let _mock = MockQueue::new()
            .keybind_action("ctrl-s", vec![6])
            .select_index(4)
            .cancel_selection()
            .guard();

        let action = run_main_menu(&db, false, &mut cursor, &mut sort)?;

        assert!(matches!(action, MenuAction::Exit));
        assert_eq!(sort, NotifySort::Application);
        let (items, _) = build_main_items(&db, false, sort)?;
        assert_eq!(cursor.initial_index(&items), Some(5));
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn read_toggle_flips_the_focused_notification_each_time() -> Result<()> {
        let (_dir, db) = populated_db()?;
        let mut cursor = MenuCursor::new();
        let mut sort = NotifySort::default();
        // Spotify (row 3) starts out read; each ctrl-u press should flip it.
        let _mock = MockQueue::new()
            .keybind_action("ctrl-u", vec![3])
            .keybind_action("ctrl-u", vec![3])
            .guard();

        let first = run_main_menu(&db, false, &mut cursor, &mut sort)?;
        assert!(matches!(
            first,
            MenuAction::ToggleRead {
                id: 4,
                read: true,
                ..
            }
        ));
        // Apply the returned action so the second press sees the new state.
        db.mark_unread(4)?;

        let second = run_main_menu(&db, false, &mut cursor, &mut sort)?;
        assert!(matches!(
            second,
            MenuAction::ToggleRead {
                id: 4,
                read: false,
                ..
            }
        ));
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn choosing_back_in_the_sort_chooser_keeps_the_order() -> Result<()> {
        let (_dir, db) = populated_db()?;
        let mut cursor = MenuCursor::new();
        let mut sort = NotifySort::default();
        // Back is the first row of the sort chooser.
        let _mock = MockQueue::new()
            .keybind_action("ctrl-s", vec![3])
            .select_index(0)
            .select_index(3)
            .guard();

        let action = run_main_menu(&db, false, &mut cursor, &mut sort)?;

        assert!(matches!(action, MenuAction::OpenNotification { id: 4, .. }));
        assert_eq!(sort, NotifySort::Newest);
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn dismissing_the_sort_chooser_keeps_the_order() -> Result<()> {
        let (_dir, db) = populated_db()?;
        let mut cursor = MenuCursor::new();
        let mut sort = NotifySort::default();
        let _mock = MockQueue::new()
            .keybind_action("ctrl-s", vec![3])
            .cancel_selection()
            .select_index(3)
            .guard();

        let action = run_main_menu(&db, false, &mut cursor, &mut sort)?;

        assert!(matches!(action, MenuAction::OpenNotification { id: 4, .. }));
        assert_eq!(sort, NotifySort::Newest);
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }
}
