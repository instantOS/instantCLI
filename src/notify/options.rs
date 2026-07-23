//! Notification options menu
//!
//! Do Not Disturb toggle, history size, and cleanup.
//! Mirrors the legacy options while detecting the active notification daemon.

use anyhow::{Context, Result};
use duct::cmd;

use crate::menu_utils::{
    ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor,
    select_one_with_style_at, select_one_with_style_at_header,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::db::{Notification, NotifyDb};

/// Run the options menu.
pub fn run_options_menu(db: &NotifyDb, _debug: bool) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let items = build_options_items();
        let initial_index = cursor.initial_index(&items);
        let selection = select_one_with_style_at(items.clone(), initial_index)?;

        match selection {
            Some(item @ OptionsItem::DoNotDisturb(_)) => {
                let backend = item.dnd_backend();
                cursor.update(&item, &items);
                if let Some(backend) = backend {
                    handle_dnd_toggle(Some(backend))?;
                }
            }
            Some(OptionsItem::DeleteByApp) => {
                cursor.update(&OptionsItem::DeleteByApp, &items);
                handle_delete_by_app(db)?;
            }
            Some(OptionsItem::DeleteByKeyword) => {
                cursor.update(&OptionsItem::DeleteByKeyword, &items);
                handle_delete_by_keyword(db)?;
            }
            Some(OptionsItem::DeleteAll) => {
                cursor.update(&OptionsItem::DeleteAll, &items);
                handle_delete_all(db)?;
            }
            Some(OptionsItem::DeleteRead) => {
                cursor.update(&OptionsItem::DeleteRead, &items);
                handle_delete_read(db)?;
            }
            Some(OptionsItem::MarkAllRead) => {
                cursor.update(&OptionsItem::MarkAllRead, &items);
                db.mark_all_read()?;
                emit(
                    Level::Success,
                    "notify.all_read",
                    &format!(
                        "{} All notifications marked as read.",
                        char::from(NerdFont::Check)
                    ),
                    None,
                );
            }
            Some(OptionsItem::HistorySize) => {
                cursor.update(&OptionsItem::HistorySize, &items);
                handle_history_size(db)?;
            }
            Some(OptionsItem::Back) | None => return Ok(()),
        }
    }
}

/// Options menu items.
#[derive(Clone)]
enum OptionsItem {
    DoNotDisturb(DndStatus),
    DeleteByApp,
    DeleteByKeyword,
    DeleteAll,
    DeleteRead,
    MarkAllRead,
    HistorySize,
    Back,
}

impl FzfSelectable for OptionsItem {
    fn fzf_display_text(&self) -> String {
        match self {
            OptionsItem::DoNotDisturb(DndStatus::Available { active, .. }) => {
                let icon = if *active {
                    format_icon_colored(NerdFont::BellSlash, colors::RED)
                } else {
                    format_icon_colored(NerdFont::Bell, colors::GREEN)
                };
                let status = if *active { "on" } else { "off" };
                format!("{icon} Do Not Disturb ({status})")
            }
            OptionsItem::DoNotDisturb(DndStatus::Unavailable) => format!(
                "{} Do Not Disturb (unavailable)",
                format_icon_colored(NerdFont::BellSlash, colors::OVERLAY1)
            ),
            OptionsItem::DeleteByApp => {
                format!(
                    "{} Delete by application",
                    format_icon_colored(NerdFont::Trash, colors::PEACH)
                )
            }
            OptionsItem::DeleteByKeyword => {
                format!(
                    "{} Delete by keyword",
                    format_icon_colored(NerdFont::Search, colors::PEACH)
                )
            }
            OptionsItem::DeleteAll => {
                format!(
                    "{} Delete all notifications",
                    format_icon_colored(NerdFont::Trash, colors::RED)
                )
            }
            OptionsItem::DeleteRead => {
                format!(
                    "{} Delete read notifications",
                    format_icon_colored(NerdFont::Check, colors::PEACH)
                )
            }
            OptionsItem::MarkAllRead => {
                format!(
                    "{} Mark all as read",
                    format_icon_colored(NerdFont::CheckDouble, colors::GREEN)
                )
            }
            OptionsItem::HistorySize => {
                format!(
                    "{} History size",
                    format_icon_colored(NerdFont::Database2, colors::BLUE)
                )
            }
            OptionsItem::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            OptionsItem::DoNotDisturb(DndStatus::Available { backend, active }) => {
                let status = if *active { "Enabled" } else { "Disabled" };
                let icon = if *active {
                    NerdFont::BellSlash
                } else {
                    NerdFont::Bell
                };
                let color = if *active { colors::RED } else { colors::GREEN };
                PreviewBuilder::new()
                    .header(icon, "Do Not Disturb")
                    .line(color, None, status)
                    .field("Notification daemon", backend.label())
                    .separator()
                    .text("Toggle Do Not Disturb mode.")
                    .blank()
                    .text("When enabled, notifications are suppressed by the notification daemon.")
                    .build()
            }
            OptionsItem::DoNotDisturb(DndStatus::Unavailable) => PreviewBuilder::new()
                .header(NerdFont::BellSlash, "Do Not Disturb Unavailable")
                .text("Neither dunstctl nor makoctl could communicate with a compatible")
                .text("running notification daemon.")
                .build(),
            OptionsItem::DeleteByApp => PreviewBuilder::new()
                .header(NerdFont::Trash, "Delete by Application")
                .text("Remove all notifications from a specific application.")
                .build(),
            OptionsItem::DeleteByKeyword => PreviewBuilder::new()
                .header(NerdFont::Search, "Delete by Keyword")
                .text("Remove notifications whose title or body contains a keyword.")
                .build(),
            OptionsItem::DeleteAll => PreviewBuilder::new()
                .header(NerdFont::Trash, "Delete All")
                .text("Remove all notifications from the database.")
                .build(),
            OptionsItem::DeleteRead => PreviewBuilder::new()
                .header(NerdFont::Check, "Delete Read")
                .text("Remove all notifications that have been read.")
                .build(),
            OptionsItem::MarkAllRead => PreviewBuilder::new()
                .header(NerdFont::CheckDouble, "Mark All Read")
                .text("Mark every notification as read.")
                .build(),
            OptionsItem::HistorySize => PreviewBuilder::new()
                .header(NerdFont::Database2, "History Size")
                .text("Set the maximum number of notifications to keep.")
                .blank()
                .text("Older notifications are automatically trimmed.")
                .build(),
            OptionsItem::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Go Back")
                .text("Return to the notification list.")
                .build(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            OptionsItem::DoNotDisturb(_) => "dnd".to_string(),
            OptionsItem::DeleteByApp => "del_app".to_string(),
            OptionsItem::DeleteByKeyword => "del_kw".to_string(),
            OptionsItem::DeleteAll => "del_all".to_string(),
            OptionsItem::DeleteRead => "del_read".to_string(),
            OptionsItem::MarkAllRead => "mark_all".to_string(),
            OptionsItem::HistorySize => "hist_size".to_string(),
            OptionsItem::Back => "__back__".to_string(),
        }
    }

    fn fzf_is_selectable(&self) -> bool {
        !matches!(self, OptionsItem::DoNotDisturb(DndStatus::Unavailable))
    }
}

impl OptionsItem {
    fn dnd_backend(&self) -> Option<DndBackend> {
        match self {
            Self::DoNotDisturb(status) => status.backend(),
            _ => None,
        }
    }
}

fn build_options_items() -> Vec<OptionsItem> {
    vec![
        OptionsItem::DoNotDisturb(detect_dnd_status()),
        OptionsItem::MarkAllRead,
        OptionsItem::DeleteByApp,
        OptionsItem::DeleteByKeyword,
        OptionsItem::DeleteRead,
        OptionsItem::DeleteAll,
        OptionsItem::HistorySize,
        OptionsItem::Back,
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DndBackend {
    Dunst,
    Mako,
}

impl DndBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Dunst => "Dunst",
            Self::Mako => "Mako",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DndStatus {
    Available { backend: DndBackend, active: bool },
    Unavailable,
}

impl DndStatus {
    fn backend(self) -> Option<DndBackend> {
        match self {
            Self::Available { backend, .. } => Some(backend),
            Self::Unavailable => None,
        }
    }
}

/// Toggle Do Not Disturb using the daemon that actually answers its control client.
fn handle_dnd_toggle(known_backend: Option<DndBackend>) -> Result<()> {
    let backend = known_backend
        .or_else(|| detect_dnd_status().backend())
        .context(
            "no supported notification daemon found (supported control clients: dunstctl, makoctl)",
        )?;
    let now_dnd = match backend {
        DndBackend::Dunst => {
            let new_state = !is_dunst_dnd()?;
            cmd!("dunstctl", "set-paused", new_state.to_string())
                .stderr_null()
                .run()
                .context("toggling dunst Do Not Disturb")?;
            new_state
        }
        DndBackend::Mako => {
            cmd!("makoctl", "mode", "-t", "do-not-disturb")
                .stderr_null()
                .run()
                .context("toggling mako Do Not Disturb")?;
            is_mako_dnd()?
        }
    };

    emit(
        Level::Success,
        "notify.dnd.toggled",
        &format!(
            "{} Do Not Disturb {}.",
            char::from(NerdFont::Bell),
            if now_dnd { "enabled" } else { "disabled" }
        ),
        None,
    );
    Ok(())
}

/// Standalone DnD toggle (for `ins notify dnd` CLI command).
pub fn run_dnd_toggle_standalone() -> Result<()> {
    handle_dnd_toggle(None)
}

/// Probe each supported daemon once and retain both its backend and current state.
fn detect_dnd_status() -> DndStatus {
    if let Ok(active) = is_dunst_dnd() {
        DndStatus::Available {
            backend: DndBackend::Dunst,
            active,
        }
    } else if let Ok(active) = is_mako_dnd() {
        DndStatus::Available {
            backend: DndBackend::Mako,
            active,
        }
    } else {
        DndStatus::Unavailable
    }
}

/// Check if mako is in do-not-disturb mode.
fn is_mako_dnd() -> Result<bool> {
    let output = cmd!("makoctl", "mode").stderr_null().read()?;
    Ok(output.lines().any(|line| line.contains("do-not-disturb")))
}

/// Check if dunst is paused (DnD).
fn is_dunst_dnd() -> Result<bool> {
    let output = cmd!("dunstctl", "is-paused").stderr_null().read()?;
    Ok(output.trim() == "true")
}

/// Handle deleting notifications by application name.
fn handle_delete_by_app(db: &NotifyDb) -> Result<()> {
    let apps = db.list_apps()?;
    if apps.is_empty() {
        emit(
            Level::Info,
            "notify.no_apps",
            &format!(
                "{} No applications with notifications.",
                char::from(NerdFont::Info)
            ),
            None,
        );
        return Ok(());
    }

    let mut items: Vec<String> = apps;
    items.push("__back__".to_string());

    // Use FzfWrapper for a simple string selection
    let result = FzfWrapper::builder()
        .prompt(format!("{} Application", char::from(NerdFont::Search)))
        .header(Header::default(
            "Select an application to delete its notifications",
        ))
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(items)?;

    let app = match result {
        FzfResult::Selected(app) if app != "__back__" => app,
        _ => return Ok(()),
    };

    let matches = db.find_by_app(&app)?;
    if matches.is_empty() {
        // Rare: the app was drawn from list_apps(), but no rows remain by the
        // time we query (e.g. deleted through another path). Stay consistent
        // with the keyword handler and tell the user instead of returning silently.
        emit(
            Level::Info,
            "notify.no_app_notifications",
            &format!(
                "{} No notifications from {app}.",
                char::from(NerdFont::Info)
            ),
            None,
        );
        return Ok(());
    }
    let scope = format!("from {app}");
    if !confirm_deletion(&matches, Some(&scope))? {
        return Ok(());
    }

    let count = db.delete_by_app(&app)?;
    emit(
        Level::Success,
        "notify.deleted_by_app",
        &format!(
            "{} Deleted {count} notifications from {app}.",
            char::from(NerdFont::Check)
        ),
        None,
    );

    Ok(())
}

/// Handle deleting notifications by keyword.
fn handle_delete_by_keyword(db: &NotifyDb) -> Result<()> {
    use crate::menu_utils::TextEditOutcome;

    let result = crate::menu_utils::prompt_text_edit(crate::menu_utils::TextEditPrompt::new(
        "Keyword to search for in title or body",
        None,
    ));

    let keyword = match result {
        // prompt_text_edit already trims and rejects empty input (showing a
        // "Clear Value / Go Back" confirm), so `keyword` here is non-empty.
        Ok(TextEditOutcome::Updated(Some(keyword))) if !keyword.is_empty() => keyword,
        _ => return Ok(()),
    };

    let matches = db.find_by_keyword(&keyword)?;
    if matches.is_empty() {
        emit(
            Level::Info,
            "notify.no_keyword_matches",
            &format!(
                "{} No notifications match '{}'.",
                char::from(NerdFont::Info),
                keyword
            ),
            None,
        );
        return Ok(());
    }

    let scope = format!("matching '{keyword}'");
    if !confirm_deletion(&matches, Some(&scope))? {
        return Ok(());
    }

    let count = db.delete_by_keyword(&keyword)?;
    emit(
        Level::Success,
        "notify.deleted_by_keyword",
        &format!(
            "{} Deleted {count} notifications containing '{keyword}'.",
            char::from(NerdFont::Check)
        ),
        None,
    );

    Ok(())
}

/// Handle deleting all read notifications.
fn handle_delete_read(db: &NotifyDb) -> Result<()> {
    let matches = db.find_read()?;
    if matches.is_empty() {
        emit(
            Level::Info,
            "notify.no_read",
            &format!(
                "{} No read notifications to delete.",
                char::from(NerdFont::Info)
            ),
            None,
        );
        return Ok(());
    }

    if !confirm_deletion(&matches, Some("that are read"))? {
        return Ok(());
    }

    let count = db.delete_read()?;
    emit(
        Level::Success,
        "notify.deleted_read",
        &format!(
            "{} Deleted {count} read notifications.",
            char::from(NerdFont::Check)
        ),
        None,
    );

    Ok(())
}

/// Handle deleting all notifications.
fn handle_delete_all(db: &NotifyDb) -> Result<()> {
    let matches = db.list()?;
    if matches.is_empty() {
        emit(
            Level::Info,
            "notify.no_notifications",
            &format!(
                "{} No notifications to delete.",
                char::from(NerdFont::Info)
            ),
            None,
        );
        return Ok(());
    }

    if !confirm_deletion(&matches, None)? {
        return Ok(());
    }

    let count = db.delete_all()?;
    emit(
        Level::Success,
        "notify.deleted_all",
        &format!(
            "{} Deleted all {count} notifications.",
            char::from(NerdFont::Check)
        ),
        None,
    );

    Ok(())
}

/// Maximum number of example notifications shown in the deletion preview.
const DELETION_PREVIEW_EXAMPLES: usize = 6;

/// Present the notifications a destructive action would remove, then require
/// explicit confirmation before proceeding.
///
/// This always gates deletion behind two steps so the user can review the
/// impact first:
/// 1. A review screen whose preview pane shows how many notifications will be
///    deleted and lists the first few examples.
/// 2. A confirmation popup restating the count and warning that the action is
///    irreversible.
///
/// `postfix` is a phrase appended after "notifications" when describing the
/// scope (e.g. `Some("matching 'battery'")`, `Some("from Discord")`, `None`
/// for deleting everything). Returns `true` only when the user confirms both
/// steps.
fn confirm_deletion(matches: &[Notification], postfix: Option<&str>) -> Result<bool> {
    if matches.is_empty() {
        return Ok(false);
    }

    let count = matches.len();
    let noun = notification_noun(count);
    let examples: Vec<Notification> = matches
        .iter()
        .take(DELETION_PREVIEW_EXAMPLES)
        .cloned()
        .collect();

    let header_text = match postfix {
        Some(scope) => format!("{count} {noun} {scope} will be deleted"),
        None => format!("{count} {noun} will be deleted"),
    };

    let items = vec![
        DeletionReview {
            count,
            postfix: postfix.map(String::from),
            examples: examples.clone(),
            action: ReviewAction::Delete,
        },
        DeletionReview {
            count,
            postfix: postfix.map(String::from),
            examples,
            action: ReviewAction::Cancel,
        },
    ];

    // Default to the delete action so the impact preview is shown immediately;
    // the confirmation popup below is the actual commit gate.
    let selection = select_one_with_style_at_header(items, Some(0), Header::default(&header_text))?;
    let Some(chosen) = selection else {
        return Ok(false);
    };
    if !chosen.action.is_delete() {
        return Ok(false);
    }

    let popup = match postfix {
        Some(scope) => format!(
            "Delete {count} {noun} {scope}?\n\nThis cannot be undone."
        ),
        None => format!("Delete {count} {noun}?\n\nThis cannot be undone."),
    };
    let result = FzfWrapper::builder()
        .confirm(popup)
        .yes_text(format!("Delete {count}"))
        .no_text("Keep")
        .confirm_dialog()?;

    Ok(matches!(result, ConfirmResult::Yes))
}

/// A single row in the deletion review screen (either the delete action or cancel).
#[derive(Clone)]
struct DeletionReview {
    count: usize,
    postfix: Option<String>,
    examples: Vec<Notification>,
    action: ReviewAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewAction {
    Delete,
    Cancel,
}

impl ReviewAction {
    fn is_delete(self) -> bool {
        matches!(self, ReviewAction::Delete)
    }
}

impl FzfSelectable for DeletionReview {
    fn fzf_display_text(&self) -> String {
        let noun = notification_noun(self.count);
        match self.action {
            ReviewAction::Delete => format!(
                "{} Delete {} {}",
                format_icon_colored(NerdFont::Trash, colors::RED),
                self.count,
                noun
            ),
            ReviewAction::Cancel => format!("{} Cancel", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        if !self.action.is_delete() {
            return PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Cancel")
                .text("No notifications will be deleted.")
                .blank()
                .text("Go back to the options menu.")
                .build();
        }

        let noun = notification_noun(self.count);
        let mut builder = PreviewBuilder::new().header(NerdFont::Trash, "Delete");
        if let Some(scope) = &self.postfix {
            builder = builder.field("Scope", scope);
        }
        builder = builder
            .field("Count", &format!("{} {}", self.count, noun))
            .line(colors::RED, Some(NerdFont::Warning), "This cannot be undone.")
            .blank()
            .separator()
            .blank()
            .subtext("Notifications that will be deleted:")
            .blank();

        for notification in &self.examples {
            builder = builder.bullet(&example_line(notification));
        }

        let shown = self.examples.len();
        if self.count > shown {
            builder = builder.subtext(&format!("…and {} more", self.count - shown));
        }

        builder.build()
    }

    fn fzf_key(&self) -> String {
        match self.action {
            ReviewAction::Delete => "__confirm_delete__".to_string(),
            ReviewAction::Cancel => "__cancel_delete__".to_string(),
        }
    }
}

/// Return "notification" or "notifications" depending on `count`.
fn notification_noun(count: usize) -> &'static str {
    if count == 1 {
        "notification"
    } else {
        "notifications"
    }
}

/// Format a notification as a single-line example for the deletion preview.
fn example_line(notification: &Notification) -> String {
    let label = notification.title.trim();
    let label = if label.is_empty() {
        notification.body.lines().next().unwrap_or("").trim()
    } else {
        label
    };
    format!("{} — {}", notification.app_name, truncate_example(label, 48))
}

/// Truncate a label to `max` visible characters, appending an ellipsis.
fn truncate_example(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Handle setting the history size limit.
fn handle_history_size(db: &NotifyDb) -> Result<()> {
    use crate::menu_utils::{TextEditOutcome, TextEditPrompt};

    let current = db.history_limit()?;
    let label = format!("Maximum notifications to keep (currently {current})");
    let prompt = TextEditPrompt::new(&label, None).ghost("1000");

    let result = crate::menu_utils::prompt_text_edit(prompt);

    match result {
        Ok(TextEditOutcome::Updated(Some(input))) if !input.is_empty() => {
            if let Ok(max) = input.parse::<usize>() {
                if max < 1 {
                    emit(
                        Level::Warn,
                        "notify.history_size.invalid",
                        &format!(
                            "{} Please enter a number greater than 0.",
                            char::from(NerdFont::Warning)
                        ),
                        None,
                    );
                    return Ok(());
                }
                let deleted = db.set_history_limit(max)?;
                emit(
                    Level::Success,
                    "notify.history_size.set",
                    &format!(
                        "{} History size set to {max}. Deleted {deleted} old notifications.",
                        char::from(NerdFont::Check)
                    ),
                    None,
                );
            } else {
                emit(
                    Level::Warn,
                    "notify.history_size.invalid",
                    &format!(
                        "{} Please enter a valid number.",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
            }
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::MockQueue;

    #[test]
    fn unavailable_dnd_control_is_visible_but_not_selectable() {
        let item = OptionsItem::DoNotDisturb(DndStatus::Unavailable);

        assert!(!item.fzf_is_selectable());
        assert!(item.fzf_display_text().contains("unavailable"));
        assert_eq!(item.fzf_key(), "dnd");
    }

    #[test]
    fn available_dnd_control_renders_from_snapshot() {
        let item = OptionsItem::DoNotDisturb(DndStatus::Available {
            backend: DndBackend::Dunst,
            active: true,
        });

        assert!(item.fzf_is_selectable());
        assert!(item.fzf_display_text().contains("(on)"));
        let FzfPreview::Text(preview) = item.fzf_preview() else {
            panic!("DND option should have a text preview");
        };
        assert!(preview.contains("Dunst"));
        assert!(preview.contains("Enabled"));
    }

    fn sample_matches(count: usize) -> Vec<Notification> {
        (0..count)
            .map(|i| Notification {
                id: i as i64,
                timestamp: "2026-07-22 12:00".to_string(),
                app_name: "Discord".to_string(),
                title: format!("Message {i}"),
                body: "hello world".to_string(),
                read: false,
                actions: vec![],
                active: true,
                invoked_action: None,
            })
            .collect()
    }

    #[test]
    fn confirm_deletion_requires_review_and_popup_confirmation() {
        // Pick "Delete" on the review screen, then confirm the popup.
        let _guard = MockQueue::new().select_index(0).confirm_yes().guard();
        let matches = sample_matches(3);

        assert!(confirm_deletion(&matches, Some("matching 'hello'")).unwrap());
    }

    #[test]
    fn confirm_deletion_aborts_when_popup_is_declined() {
        let _guard = MockQueue::new().select_index(0).confirm_no().guard();
        let matches = sample_matches(2);

        assert!(!confirm_deletion(&matches, Some("matching 'hello'")).unwrap());
    }

    #[test]
    fn confirm_deletion_aborts_when_review_screen_is_cancelled() {
        let _guard = MockQueue::new().cancel_selection().guard();
        let matches = sample_matches(2);

        assert!(!confirm_deletion(&matches, None).unwrap());
    }

    #[test]
    fn confirm_deletion_aborts_when_cancel_action_is_chosen() {
        // Index 1 is the "Cancel" action on the review screen.
        let _guard = MockQueue::new().select_index(1).guard();
        let matches = sample_matches(2);

        assert!(!confirm_deletion(&matches, None).unwrap());
    }

    #[test]
    fn confirm_deletion_returns_false_without_prompting_when_empty() {
        // No mocks queued: an empty match list short-circuits before any dialog.
        assert!(!confirm_deletion(&[], None).unwrap());
    }

    #[test]
    fn deletion_review_preview_shows_count_and_examples() {
        let review = DeletionReview {
            count: 9,
            postfix: Some("matching 'hello'".to_string()),
            examples: sample_matches(4),
            action: ReviewAction::Delete,
        };

        let FzfPreview::Text(preview) = review.fzf_preview() else {
            panic!("delete review should render a text preview");
        };
        assert!(preview.contains("matching 'hello'"));
        assert!(preview.contains("9 notifications"));
        // Examples are rendered as bullets.
        assert!(preview.contains("Discord — Message 0"));
        // Remaining count beyond the capped examples.
        assert!(preview.contains("and 5 more"));
    }

    #[test]
    fn deletion_review_preview_has_no_overflow_when_all_examples_shown() {
        let review = DeletionReview {
            count: 3,
            postfix: None,
            examples: sample_matches(3),
            action: ReviewAction::Delete,
        };

        let FzfPreview::Text(preview) = review.fzf_preview() else {
            panic!("delete review should render a text preview");
        };
        assert!(!preview.contains("more"));
    }
}
