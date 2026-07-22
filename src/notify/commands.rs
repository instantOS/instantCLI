//! CLI commands for the notification center
//!
//! Subcommands for `ins notify`:
//! - No subcommand → interactive FZF notification browser
//! - `list` → list notifications (text/json)
//! - `count` → unread count
//! - `delete` → delete by app, keyword, read, all, or specific ID
//! - `read` → mark as read
//! - `unread` → mark as unread
//! - `dnd` → toggle Do Not Disturb
//! - `daemon` → start D-Bus capture daemon

use anyhow::Result;
use clap::{ArgGroup, Subcommand};

use crate::ui::prelude::*;

use super::db::NotifyDb;

/// Notification center subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum NotifyCommands {
    /// List all notifications
    List {
        /// Show only unread notifications
        #[arg(long)]
        unread_only: bool,
    },

    /// Show count of unread notifications
    Count,

    /// Delete notifications
    #[command(group(
        ArgGroup::new("criteria")
            .required(true)
            .multiple(false)
            .args(["app", "keyword", "read", "all", "id"])
    ))]
    Delete {
        /// Delete notifications from a specific application
        #[arg(long = "app")]
        app: Option<String>,

        /// Delete notifications containing a keyword
        #[arg(long = "keyword")]
        keyword: Option<String>,

        /// Delete all read notifications
        #[arg(long = "read")]
        read: bool,

        /// Delete all notifications
        #[arg(long = "all")]
        all: bool,

        /// Delete a specific notification by ID
        #[arg(long = "id")]
        id: Option<i64>,
    },

    /// Mark a notification as read
    Read {
        /// Notification ID (use "all" to mark all as read)
        id: String,
    },

    /// Mark a notification as unread
    Unread {
        /// Notification ID
        id: i64,
    },

    /// Toggle Do Not Disturb mode
    Dnd,

    /// Start the D-Bus notification capture daemon
    Daemon,
}

/// Handle a notification subcommand.
pub async fn handle_notify_command(command: &Option<NotifyCommands>, debug: bool) -> Result<()> {
    match command {
        None => super::menu::run_notify_ui(debug),
        Some(NotifyCommands::List { unread_only }) => list_notifications(*unread_only),
        Some(NotifyCommands::Count) => show_count(),
        Some(NotifyCommands::Delete {
            app,
            keyword,
            read,
            all,
            id,
        }) => handle_delete(app.as_deref(), keyword.as_deref(), *read, *all, *id),
        Some(NotifyCommands::Read { id }) => handle_mark_read(id),
        Some(NotifyCommands::Unread { id }) => handle_mark_unread(*id),
        Some(NotifyCommands::Dnd) => super::options::run_dnd_toggle_standalone(),
        Some(NotifyCommands::Daemon) => super::capture::run_daemon(debug).await,
    }
}

fn list_notifications(unread_only: bool) -> Result<()> {
    let db = NotifyDb::open()?;
    let notifications = db.list()?;

    let filtered: Vec<_> = if unread_only {
        notifications.into_iter().filter(|n| !n.read).collect()
    } else {
        notifications
    };

    let format = get_output_format();
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&filtered)?;
            println!("{json}");
        }
        OutputFormat::Text => {
            if filtered.is_empty() {
                emit(
                    Level::Info,
                    "notify.list.empty",
                    &format!(
                        "{} No notifications.",
                        char::from(crate::ui::nerd_font::NerdFont::Bell)
                    ),
                    None,
                );
            } else {
                for n in &filtered {
                    let read_icon = if n.read {
                        " ".to_string()
                    } else {
                        char::from(crate::ui::nerd_font::NerdFont::Circle).to_string()
                    };
                    println!(
                        "[{read_icon}] {:>5} {:>8} {}",
                        n.id, n.timestamp, n.app_name
                    );
                    println!("        {}", n.title);
                    if !n.body.is_empty() {
                        // Indent and wrap body
                        for line in n.body.lines() {
                            println!("        {line}");
                        }
                    }
                    println!();
                }
            }
        }
    }

    Ok(())
}

fn show_count() -> Result<()> {
    let db = NotifyDb::open()?;
    let count = db.unread_count()?;

    let format = get_output_format();
    match format {
        OutputFormat::Json => {
            println!("{{\"unread\": {count}}}");
        }
        OutputFormat::Text => {
            println!("{count}");
        }
    }

    Ok(())
}

fn handle_delete(
    app: Option<&str>,
    keyword: Option<&str>,
    read: bool,
    all: bool,
    id: Option<i64>,
) -> Result<()> {
    let db = NotifyDb::open()?;

    if let Some(id) = id {
        anyhow::ensure!(db.delete(id)?, "notification {id} was not found");
        emit(
            Level::Success,
            "notify.deleted",
            &format!(
                "{} Deleted notification {id}.",
                char::from(crate::ui::nerd_font::NerdFont::Check)
            ),
            None,
        );
    } else if all {
        let count = db.delete_all()?;
        emit(
            Level::Success,
            "notify.deleted_all",
            &format!(
                "{} Deleted all {count} notifications.",
                char::from(crate::ui::nerd_font::NerdFont::Check)
            ),
            None,
        );
    } else if read {
        let count = db.delete_read()?;
        emit(
            Level::Success,
            "notify.deleted_read",
            &format!(
                "{} Deleted {count} read notifications.",
                char::from(crate::ui::nerd_font::NerdFont::Check)
            ),
            None,
        );
    } else if let Some(app_name) = app {
        let count = db.delete_by_app(app_name)?;
        emit(
            Level::Success,
            "notify.deleted_by_app",
            &format!(
                "{} Deleted {count} notifications from {app_name}.",
                char::from(crate::ui::nerd_font::NerdFont::Check)
            ),
            None,
        );
    } else if let Some(kw) = keyword {
        let count = db.delete_by_keyword(kw)?;
        emit(
            Level::Success,
            "notify.deleted_by_keyword",
            &format!(
                "{} Deleted {count} notifications containing '{kw}'.",
                char::from(crate::ui::nerd_font::NerdFont::Check)
            ),
            None,
        );
    }

    Ok(())
}

fn handle_mark_read(id: &str) -> Result<()> {
    let db = NotifyDb::open()?;

    if id == "all" {
        db.mark_all_read()?;
        emit(
            Level::Success,
            "notify.all_read",
            &format!(
                "{} All notifications marked as read.",
                char::from(crate::ui::nerd_font::NerdFont::Check)
            ),
            None,
        );
    } else {
        let id: i64 = id
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid notification ID: {id}"))?;
        anyhow::ensure!(db.mark_read(id)?, "notification {id} was not found");
        emit(
            Level::Success,
            "notify.marked_read",
            &format!(
                "{} Notification {id} marked as read.",
                char::from(crate::ui::nerd_font::NerdFont::Check)
            ),
            None,
        );
    }

    Ok(())
}

fn handle_mark_unread(id: i64) -> Result<()> {
    let db = NotifyDb::open()?;
    anyhow::ensure!(db.mark_unread(id)?, "notification {id} was not found");
    emit(
        Level::Success,
        "notify.marked_unread",
        &format!(
            "{} Notification {id} marked as unread.",
            char::from(crate::ui::nerd_font::NerdFont::Check)
        ),
        None,
    );
    Ok(())
}
