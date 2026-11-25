//! Ordering of the notifications listed in the notification center.

use crate::menu_utils::{MenuSort, compare_names};
use crate::ui::nerd_font::NerdFont;

use super::items::NotificationListItem;

/// How the notifications in the notification center are ordered.
///
/// The menu's fixed entries (Close, Options, ...) always stay above the
/// notifications; only the notifications move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifySort {
    /// The latest notification first.
    Newest,
    /// The earliest notification first.
    Oldest,
    /// Notifications that have not been read first.
    Unread,
    /// Grouped by the application that sent them, from A to Z.
    Application,
}

impl Default for NotifySort {
    /// The notification center opens with the latest notification first.
    fn default() -> Self {
        Self::Newest
    }
}

impl MenuSort for NotifySort {
    const ALL: &'static [Self] = &[Self::Newest, Self::Oldest, Self::Unread, Self::Application];

    const CHOOSER_TITLE: &'static str = "Sort Notifications";

    const CHOOSER_SUBTITLE: &'static str = "Choose the order of the notification list.";

    fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest first",
            Self::Oldest => "Oldest first",
            Self::Unread => "Unread first",
            Self::Application => "By application",
        }
    }

    fn icon(self) -> NerdFont {
        match self {
            Self::Newest => NerdFont::ArrowDown,
            Self::Oldest => NerdFont::ArrowUp,
            Self::Unread => NerdFont::EnvelopeOpen,
            Self::Application => NerdFont::Application,
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Self::Newest => "The latest notification is listed first.",
            Self::Oldest => "The earliest notification is listed first.",
            Self::Unread => "Notifications you have not read are listed first.",
            Self::Application => "Notifications are grouped by the application that sent them.",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Newest | Self::Oldest => "Applications are interleaved as they notified you.",
            Self::Unread => "Each group lists the latest notification first.",
            Self::Application => {
                "Applications from A to Z, the latest notification first in each group."
            }
        }
    }
}

/// Order the menu's notification rows.
///
/// Rows arrive newest first and are compared by ID, which follows arrival, so
/// [`NotifySort::Newest`] keeps that order. The sort is stable, so equal
/// notifications keep their incoming order.
pub fn sort_notifications(
    mut notifications: Vec<NotificationListItem>,
    sort: NotifySort,
) -> Vec<NotificationListItem> {
    notifications.sort_by(|left, right| {
        let newest_first = right.id.cmp(&left.id);
        match sort {
            NotifySort::Newest => newest_first,
            NotifySort::Oldest => left.id.cmp(&right.id),
            NotifySort::Unread => left.read.cmp(&right.read).then(newest_first),
            NotifySort::Application => {
                compare_names(&left.app_name, &right.app_name).then(newest_first)
            }
        }
    });
    notifications
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, app_name: &str, read: bool) -> NotificationListItem {
        NotificationListItem {
            id,
            app_name: app_name.to_string(),
            title: String::new(),
            body: String::new(),
            timestamp: String::new(),
            read,
        }
    }

    fn ids(notifications: &[NotificationListItem]) -> Vec<i64> {
        notifications
            .iter()
            .map(|notification| notification.id)
            .collect()
    }

    #[test]
    fn newest_keeps_the_arrival_order() {
        // The database lists rows newest first.
        let ordered = sort_notifications(
            vec![
                item(4, "Spotify", true),
                item(3, "Discord", false),
                item(2, "Discord", true),
                item(1, "Slack", false),
            ],
            NotifySort::Newest,
        );

        assert_eq!(ids(&ordered), [4, 3, 2, 1]);
    }

    #[test]
    fn oldest_reverses_the_arrival_order() {
        let ordered = sort_notifications(
            vec![
                item(4, "Spotify", true),
                item(3, "Discord", false),
                item(2, "Discord", true),
                item(1, "Slack", false),
            ],
            NotifySort::Oldest,
        );

        assert_eq!(ids(&ordered), [1, 2, 3, 4]);
    }

    #[test]
    fn unread_lists_unread_notifications_first() {
        let ordered = sort_notifications(
            vec![
                item(4, "Spotify", true),
                item(3, "Discord", false),
                item(2, "Discord", true),
                item(1, "Slack", false),
            ],
            NotifySort::Unread,
        );

        assert_eq!(ids(&ordered), [3, 1, 4, 2]);
    }

    #[test]
    fn application_groups_notifications_ignoring_case() {
        let ordered = sort_notifications(
            vec![
                item(4, "Zebra", false),
                item(3, "apple", false),
                item(2, "Zebra", false),
                item(1, "apple", false),
            ],
            NotifySort::Application,
        );

        // apple comes before Zebra even though it is lowercase, and each
        // application lists its latest notification first.
        assert_eq!(ids(&ordered), [3, 1, 4, 2]);
    }

    #[test]
    fn newest_is_the_default_order() {
        assert_eq!(NotifySort::default(), NotifySort::Newest);
    }
}
