//! Notification options menu
//!
//! Do Not Disturb toggle, history size, and cleanup.
//! Mirrors the legacy options while detecting the active notification daemon.

use anyhow::{Context, Result};
use duct::cmd;

use crate::menu_utils::{
    FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor, select_one_with_style_at,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::db::NotifyDb;

/// Run the options menu.
pub fn run_options_menu(db: &NotifyDb, _debug: bool) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let items = build_options_items(db)?;
        let initial_index = cursor.initial_index(&items);
        let selection = select_one_with_style_at(items.clone(), initial_index)?;

        match selection {
            Some(OptionsItem::DoNotDisturb) => {
                cursor.update(&OptionsItem::DoNotDisturb, &items);
                handle_dnd_toggle()?;
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
    DoNotDisturb,
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
            OptionsItem::DoNotDisturb => {
                let is_dnd = is_dnd_active();
                let icon = if is_dnd {
                    format_icon_colored(NerdFont::BellSlash, colors::RED)
                } else {
                    format_icon_colored(NerdFont::Bell, colors::GREEN)
                };
                let status = if is_dnd { "on" } else { "off" };
                format!("{icon} Do Not Disturb ({status})")
            }
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
            OptionsItem::DoNotDisturb => {
                let is_dnd = is_dnd_active();
                let status = if is_dnd { "Enabled" } else { "Disabled" };
                let icon = if is_dnd {
                    NerdFont::BellSlash
                } else {
                    NerdFont::Bell
                };
                let color = if is_dnd { colors::RED } else { colors::GREEN };
                PreviewBuilder::new()
                    .line(color, Some(icon), status)
                    .separator()
                    .text("Toggle Do Not Disturb mode.")
                    .blank()
                    .text("When enabled, notifications are suppressed by the notification daemon.")
                    .build()
            }
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
            OptionsItem::DoNotDisturb => "dnd".to_string(),
            OptionsItem::DeleteByApp => "del_app".to_string(),
            OptionsItem::DeleteByKeyword => "del_kw".to_string(),
            OptionsItem::DeleteAll => "del_all".to_string(),
            OptionsItem::DeleteRead => "del_read".to_string(),
            OptionsItem::MarkAllRead => "mark_all".to_string(),
            OptionsItem::HistorySize => "hist_size".to_string(),
            OptionsItem::Back => "__back__".to_string(),
        }
    }
}

fn build_options_items(_db: &NotifyDb) -> Result<Vec<OptionsItem>> {
    Ok(vec![
        OptionsItem::DoNotDisturb,
        OptionsItem::MarkAllRead,
        OptionsItem::DeleteByApp,
        OptionsItem::DeleteByKeyword,
        OptionsItem::DeleteRead,
        OptionsItem::DeleteAll,
        OptionsItem::HistorySize,
        OptionsItem::Back,
    ])
}

#[derive(Clone, Copy)]
enum DndBackend {
    Dunst,
    Mako,
}

/// Toggle Do Not Disturb using the daemon that actually answers its control client.
fn handle_dnd_toggle() -> Result<()> {
    let backend = detect_dnd_backend().context(
        "no supported notification daemon found (supported control clients: dunstctl, makoctl)",
    )?;
    let now_dnd = match backend {
        DndBackend::Dunst => {
            let new_state = !is_dunst_dnd()?;
            cmd!("dunstctl", "set-paused", new_state.to_string())
                .run()
                .context("toggling dunst Do Not Disturb")?;
            new_state
        }
        DndBackend::Mako => {
            cmd!("makoctl", "mode", "-t", "do-not-disturb")
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
    handle_dnd_toggle()
}

/// Check if DnD is currently active.
fn is_dnd_active() -> bool {
    match detect_dnd_backend() {
        Some(DndBackend::Dunst) => is_dunst_dnd().unwrap_or(false),
        Some(DndBackend::Mako) => is_mako_dnd().unwrap_or(false),
        None => false,
    }
}

fn detect_dnd_backend() -> Option<DndBackend> {
    if is_dunst_dnd().is_ok() {
        Some(DndBackend::Dunst)
    } else if is_mako_dnd().is_ok() {
        Some(DndBackend::Mako)
    } else {
        None
    }
}

/// Check if mako is in do-not-disturb mode.
fn is_mako_dnd() -> Result<bool> {
    let output = cmd!("makoctl", "mode").read()?;
    Ok(output.lines().any(|line| line.contains("do-not-disturb")))
}

/// Check if dunst is paused (DnD).
fn is_dunst_dnd() -> Result<bool> {
    let output = cmd!("dunstctl", "is-paused").read()?;
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

    match result {
        FzfResult::Selected(app) if app != "__back__" => {
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
        }
        _ => {}
    }

    Ok(())
}

/// Handle deleting notifications by keyword.
fn handle_delete_by_keyword(db: &NotifyDb) -> Result<()> {
    use crate::menu_utils::TextEditOutcome;

    let result = crate::menu_utils::prompt_text_edit(crate::menu_utils::TextEditPrompt::new(
        "Keyword to search for in title or body",
        None,
    ));

    match result {
        Ok(TextEditOutcome::Updated(Some(keyword))) if !keyword.is_empty() => {
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
        }
        _ => {}
    }

    Ok(())
}

/// Handle deleting all notifications with confirmation.
fn handle_delete_all(db: &NotifyDb) -> Result<()> {
    use crate::menu_utils::ConfirmResult;

    let result = FzfWrapper::builder()
        .confirm("Delete all notifications?")
        .confirm_dialog()?;

    if let ConfirmResult::Yes = result {
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
    }

    Ok(())
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
