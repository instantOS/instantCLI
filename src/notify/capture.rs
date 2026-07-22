//! D-Bus notification history monitor.
//!
//! Observes `org.freedesktop.Notifications.Notify` calls without replacing the
//! user's notification daemon. Calls and replies are correlated so that
//! `replaces_id` updates replace an existing history row instead of creating
//! an unbounded stream of progress notifications.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::Local;
use zbus::Message;
use zbus::connection::Builder;
use zbus::fdo::RequestNameFlags;
use zbus::message::Type as MessageType;
use zbus::zvariant::OwnedValue;

use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

use super::db::NotifyDb;

type PendingCalls = HashMap<(String, u32), i64>;
type NotifyArgs = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    HashMap<String, OwnedValue>,
    i32,
);

/// Run the notification capture daemon until the D-Bus connection closes.
pub async fn run_daemon(debug: bool) -> Result<()> {
    emit(
        Level::Info,
        "notify.daemon.start",
        &format!(
            "{} Starting notification history daemon...",
            char::from(NerdFont::Bell)
        ),
        None,
    );

    let config = NotificationConfig::load()?;
    let db = NotifyDb::open()?;
    let mut pending = PendingCalls::new();

    // Keep a separate connection holding the name because BecomeMonitor turns
    // its connection into a monitor-only connection and may release names.
    let instance_guard = Builder::session()?
        .build()
        .await
        .context("connecting notification history daemon instance guard")?;
    instance_guard
        .request_name_with_flags(
            "org.instantos.NotificationHistory",
            RequestNameFlags::DoNotQueue.into(),
        )
        .await
        .context("another notification history daemon is already running")?;
    let connection = Builder::session()?
        .build()
        .await
        .context("connecting notification history monitor to D-Bus")?;

    // BecomeMonitor works with both dbus-daemon and dbus-broker. Replies do
    // not carry the original interface/member, so observe all replies and
    // correlate only serials belonging to Notify calls that we recorded.
    let notify_rule =
        "type='method_call',interface='org.freedesktop.Notifications',member='Notify'";
    let return_rule = "type='method_return'";
    let error_rule = "type='error'";
    let match_rules: &[&str] = &[notify_rule, return_rule, error_rule];
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(match_rules, 0u32),
        )
        .await
        .context("becoming D-Bus monitor for notifications")?;

    emit(
        Level::Success,
        "notify.daemon.listening",
        &format!(
            "{} Listening for notifications on D-Bus.",
            char::from(NerdFont::Check)
        ),
        None,
    );
    if debug {
        emit(
            Level::Debug,
            "notify.daemon.debug",
            &format!("D-Bus monitor installed: match_rule='{notify_rule}'"),
            None,
        );
    }

    use futures_util::StreamExt;
    let mut stream = zbus::MessageStream::from(&connection);
    while let Some(message) = stream.next().await {
        match message {
            Ok(message) => {
                if let Err(error) = handle_message(&message, &db, &config, &mut pending, debug) {
                    emit(
                        Level::Warn,
                        "notify.daemon.error",
                        &format!(
                            "{} Error handling notification: {error}",
                            char::from(NerdFont::Warning)
                        ),
                        None,
                    );
                }
            }
            Err(error) => emit(
                Level::Warn,
                "notify.daemon.error",
                &format!(
                    "{} D-Bus stream error: {error}",
                    char::from(NerdFont::Warning)
                ),
                None,
            ),
        }
    }

    drop(instance_guard);
    anyhow::bail!("notification D-Bus monitor ended unexpectedly")
}

fn handle_message(
    message: &Message,
    db: &NotifyDb,
    config: &NotificationConfig,
    pending: &mut PendingCalls,
    debug: bool,
) -> Result<()> {
    if matches!(
        message.message_type(),
        MessageType::MethodReturn | MessageType::Error
    ) {
        return handle_reply(message, db, pending, debug);
    }

    let header = message.header();
    if header.member().map(|member| member.as_str()) != Some("Notify")
        || header.interface().map(|interface| interface.as_str())
            != Some("org.freedesktop.Notifications")
    {
        return Ok(());
    }

    let (app_name, replaces_id, _icon, summary, body, _actions, hints, _timeout): NotifyArgs =
        message
            .body()
            .deserialize()
            .context("deserializing Notify message body")?;

    let transient = hints
        .get("transient")
        .and_then(|value| value.downcast_ref::<bool>().ok())
        .unwrap_or(false);
    if transient || config.is_ignored(&app_name) {
        if debug {
            emit(
                Level::Debug,
                "notify.daemon.ignored",
                &format!("Ignored transient or configured notification from {app_name}"),
                None,
            );
        }
        return Ok(());
    }

    let sender = header
        .sender()
        .context("notification method call has no D-Bus sender")?
        .as_str()
        .to_owned();
    let app_name = sanitize_text(&app_name, 100);
    let summary = sanitize_text(&summary, 300);
    let body = sanitize_text(&body, 2000);
    let timestamp = Local::now().format("%Y-%m-%d %H:%M").to_string();

    let local_id = if replaces_id == 0 {
        let id = db.add_captured(&timestamp, &app_name, &summary, &body, None, None)?;
        if pending.len() >= 4096 {
            pending.clear();
        }
        pending.insert((sender, message.primary_header().serial_num().get()), id);
        id
    } else if let Some(id) =
        db.replace_captured(&timestamp, &app_name, &summary, &body, &sender, replaces_id)?
    {
        id
    } else {
        db.add_captured(
            &timestamp,
            &app_name,
            &summary,
            &body,
            Some(&sender),
            Some(replaces_id),
        )?
    };

    if debug {
        emit(
            Level::Debug,
            "notify.daemon.captured",
            &format!("Captured {local_id}: [{app_name}] {summary}: {body}"),
            None,
        );
    }
    Ok(())
}

fn handle_reply(
    message: &Message,
    db: &NotifyDb,
    pending: &mut PendingCalls,
    debug: bool,
) -> Result<()> {
    let header = message.header();
    let (Some(reply_serial), Some(destination)) = (header.reply_serial(), header.destination())
    else {
        return Ok(());
    };
    let key = (destination.as_str().to_owned(), reply_serial.get());
    let Some(local_id) = pending.remove(&key) else {
        return Ok(());
    };

    if message.message_type() == MessageType::MethodReturn {
        let external_id: u32 = message
            .body()
            .deserialize()
            .context("deserializing notification ID reply")?;
        db.assign_external_id(local_id, &key.0, external_id)?;
        if debug {
            emit(
                Level::Debug,
                "notify.daemon.correlated",
                &format!("Correlated local notification {local_id} with D-Bus ID {external_id}"),
                None,
            );
        }
    }
    Ok(())
}

fn sanitize_text(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(max_chars)
        .collect()
}

struct NotificationConfig {
    ignore_apps: HashSet<String>,
}

impl NotificationConfig {
    fn load() -> Result<Self> {
        let ignore_path = crate::common::paths::instant_config_dir()?.join("notifyignore");
        Ok(Self {
            ignore_apps: load_app_list(&ignore_path),
        })
    }

    fn is_ignored(&self, app_name: &str) -> bool {
        self.ignore_apps
            .iter()
            .any(|ignored| ignored.eq_ignore_ascii_case(app_name))
    }
}

fn load_app_list(path: &std::path::Path) -> HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize_text;

    #[test]
    fn sanitize_text_is_unicode_safe_and_removes_controls() {
        let value = sanitize_text("hello\u{1b}[31m 😀世界", 13);
        assert_eq!(value, "hello[31m 😀世界");
    }

    #[test]
    fn sanitize_text_truncates_by_character() {
        assert_eq!(sanitize_text("😀😀😀", 2), "😀😀");
    }
}
