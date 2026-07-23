//! FZF menu items for the notification center
//!
//! Implements `FzfSelectable` for notification lists and detail views,
//! following the same pattern as `settings/ui/items.rs`.

use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

const RESET: &str = "\x1b[0m";

/// A notification item for display in the main FZF menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationListItem {
    pub id: i64,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub timestamp: String,
    pub read: bool,
}

/// Top-level menu items in the notification center.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyMainItem {
    /// A notification entry.
    Notification(NotificationListItem),
    /// Enable the background capture service.
    EnableCapture,
    /// Options submenu.
    Options,
    /// Close the notification center.
    Close,
}

impl FzfSelectable for NotifyMainItem {
    fn fzf_display_text(&self) -> String {
        match self {
            NotifyMainItem::Notification(n) => {
                let icon = if n.read {
                    format_icon_colored(NerdFont::Envelope, colors::OVERLAY1)
                } else {
                    format_icon_colored(NerdFont::EnvelopeOpen, colors::YELLOW)
                };

                let app_color = hex_to_ansi_fg(colors::BLUE);
                let title_color = hex_to_ansi_fg(colors::TEXT);
                let time_color = hex_to_ansi_fg(colors::SUBTEXT0);

                format!(
                    "{} {app_color}{}{RESET} {title_color}{}{RESET} {time_color}({}){RESET}",
                    icon, n.app_name, n.title, n.timestamp,
                )
            }
            NotifyMainItem::EnableCapture => format!(
                "{} Enable and start notification capture",
                format_icon_colored(NerdFont::PlayCircle, colors::GREEN)
            ),
            NotifyMainItem::Options => {
                format!(
                    "{} Options",
                    format_icon_colored(NerdFont::Gear, colors::MAUVE)
                )
            }
            NotifyMainItem::Close => format!("{} Close", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            NotifyMainItem::Notification(n) => {
                let read_label = if n.read { "Read" } else { "Unread" };
                let read_icon = if n.read {
                    NerdFont::Envelope
                } else {
                    NerdFont::EnvelopeOpen
                };
                let read_color = if n.read {
                    colors::OVERLAY1
                } else {
                    colors::YELLOW
                };

                let mut builder = PreviewBuilder::new()
                    .line(read_color, Some(read_icon), &n.title)
                    .separator()
                    .blank()
                    .field("Application", &n.app_name)
                    .field("Time", &n.timestamp)
                    .field("Status", read_label)
                    .blank()
                    .separator()
                    .blank();

                for line in wrap_text(&n.body, 60) {
                    builder = builder.text(&line);
                }

                builder.build()
            }
            NotifyMainItem::EnableCapture => PreviewBuilder::new()
                .header(NerdFont::PlayCircle, "Enable Notification Capture")
                .text("Start the supervised background capture service now")
                .text("and automatically on future graphical logins.")
                .blank()
                .text("No notification history is recorded while capture is stopped.")
                .build(),
            NotifyMainItem::Options => PreviewBuilder::new()
                .header(NerdFont::Gear, "Notification Options")
                .text("Configure Do Not Disturb, clear notifications,")
                .text("delete by app/keyword, and adjust history size.")
                .build(),
            NotifyMainItem::Close => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close")
                .text("Exit the notification center.")
                .build(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            NotifyMainItem::Notification(n) => format!("notif:{}", n.id),
            NotifyMainItem::EnableCapture => "__enable_capture__".to_string(),
            NotifyMainItem::Options => "__options__".to_string(),
            NotifyMainItem::Close => "__close__".to_string(),
        }
    }
}

/// Actions available in the notification detail view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationDetailAction {
    Back,
    MarkUnread,
    Delete,
}

/// Notification content shown while a detail action is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationDetailPreview {
    pub title: String,
    pub app_name: String,
    pub timestamp: String,
    pub body: String,
    pub actions: Option<String>,
}

/// An actionable item in the notification detail submenu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationDetailItem {
    pub(super) action: NotificationDetailAction,
    pub(super) preview: NotificationDetailPreview,
}

impl FzfSelectable for NotificationDetailItem {
    fn fzf_display_text(&self) -> String {
        match self.action {
            NotificationDetailAction::Back => format!("{} Back", format_back_icon()),
            NotificationDetailAction::MarkUnread => {
                format!(
                    "{} Mark as unread",
                    format_icon_colored(NerdFont::EnvelopeOpen, colors::YELLOW)
                )
            }
            NotificationDetailAction::Delete => {
                format!(
                    "{} Delete",
                    format_icon_colored(NerdFont::Trash, colors::RED)
                )
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        detail_action_preview(&self.action, &self.preview)
    }

    fn fzf_key(&self) -> String {
        match self.action {
            NotificationDetailAction::Back => "__back__".to_string(),
            NotificationDetailAction::MarkUnread => "__mark_unread__".to_string(),
            NotificationDetailAction::Delete => "__delete__".to_string(),
        }
    }
}

fn detail_action_preview(
    action: &NotificationDetailAction,
    notification: &NotificationDetailPreview,
) -> FzfPreview {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Envelope, &notification.title)
        .field("Application", &notification.app_name)
        .field("Time", &notification.timestamp);

    if let Some(actions) = &notification.actions {
        builder = builder.field("Actions", actions);
    }

    builder = builder.blank().separator().blank();
    for line in wrap_text(&notification.body, 60) {
        builder = builder.text(&line);
    }

    let (icon, label, description) = match action {
        NotificationDetailAction::Back => (
            NerdFont::ArrowLeft,
            "Back",
            "Return to the notification list.",
        ),
        NotificationDetailAction::MarkUnread => (
            NerdFont::EnvelopeOpen,
            "Mark as Unread",
            "Mark this notification as unread.",
        ),
        NotificationDetailAction::Delete => (
            NerdFont::Trash,
            "Delete",
            "Remove this notification from the database.",
        ),
    };

    builder
        .blank()
        .separator()
        .blank()
        .line(colors::SUBTEXT0, Some(icon), label)
        .text(description)
        .build()
}

/// Wrap text to a maximum line width, preserving existing newlines.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut characters = line.chars().peekable();
        while characters.peek().is_some() {
            result.push(characters.by_ref().take(width).collect());
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::wrap_text;

    #[test]
    fn wrap_text_handles_multibyte_characters() {
        assert_eq!(wrap_text("😀世界abc", 2), vec!["😀世", "界a", "bc"]);
    }

    #[test]
    fn wrap_text_preserves_blank_lines() {
        assert_eq!(wrap_text("one\n\ntwo", 10), vec!["one", "", "two"]);
    }
}
