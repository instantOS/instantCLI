//! D-Bus notification capture daemon
//!
//! Eavesdrops on `org.freedesktop.Notifications.Notify` method calls on the
//! D-Bus session bus. This works with any notification daemon (dunst, mako,
//! swaync, etc.) and on both X11 and Wayland, because all notifications flow
//! through the same D-Bus interface regardless of display server.
//!
//! The daemon stores each captured notification in the SQLite database,
//! replacing the legacy `instantnotifytrigger.sh` approach that relied on
//! dunst's notification hook (which mako doesn't support).

use anyhow::{Context, Result};
use chrono::Local;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::connection::Builder;
use zbus::proxy::CacheProperties;
use zbus::Message;

use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

use super::db::NotifyDb;

/// Run the notification capture daemon.
///
/// Listens on the D-Bus session bus for `Notify` method calls and stores
/// them in the notification database. Runs until interrupted by Ctrl-C or
/// a signal.
pub async fn run_daemon(debug: bool) -> Result<()> {
    emit(
        Level::Info,
        "notify.daemon.start",
        &format!("{} Starting notification daemon...", char::from(NerdFont::Bell)),
        None,
    );

    // Load ignore/silent lists
    let config = Arc::new(NotificationConfig::load()?);

    // Open the database
    let db = Arc::new(Mutex::new(NotifyDb::open()?));

    // Connect to the session bus
    let connection = Builder::session()?
        .build()
        .await
        .context("connecting to D-Bus session bus")?;

    // Add a match rule to eavesdrop on Notify method calls
    // We use eavesdrop=true because we're listening to method calls
    // destined for another service (the notification daemon)
    let dbus_proxy: zbus::Proxy = zbus::proxy::Builder::new(&connection)
        .destination("org.freedesktop.DBus")?
        .path("/org/freedesktop/DBus")?
        .interface("org.freedesktop.DBus")?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .context("creating D-Bus proxy")?;

    // Add match rule for eavesdropping on Notify method calls
    // type='method_call' ensures we only catch calls, not returns
    // eavesdrop='true' allows receiving messages not addressed to us
    dbus_proxy
        .call::<_, _, ()>(
            "AddMatch",
            &(
                "type='method_call',interface='org.freedesktop.Notifications',member='Notify',eavesdrop='true'",
            ),
        )
        .await
        .context("adding D-Bus match rule for Notify")?;

    emit(
        Level::Success,
        "notify.daemon.listening",
        &format!("{} Listening for notifications on D-Bus.", char::from(NerdFont::Check)),
        None,
    );

    if debug {
        emit(
            Level::Debug,
            "notify.daemon.debug",
            "D-Bus match rule installed: type='method_call',interface='org.freedesktop.Notifications',member='Notify',eavesdrop='true'",
            None,
        );
    }

    use futures_util::StreamExt;
    let mut stream = zbus::MessageStream::from(&connection);

    while let Some(msg_result) = stream.next().await {
        match msg_result {
            Ok(msg) => {
                if let Err(e) = handle_message(&msg, &db, &config, debug).await {
                    emit(
                        Level::Warn,
                        "notify.daemon.error",
                        &format!("{} Error handling notification: {e}", char::from(NerdFont::Warning)),
                        None,
                    );
                }
            }
            Err(e) => {
                emit(
                    Level::Warn,
                    "notify.daemon.error",
                    &format!("{} D-Bus stream error: {e}", char::from(NerdFont::Warning)),
                    None,
                );
            }
        }
    }

    Ok(())
}

/// Handle a single D-Bus message.
async fn handle_message(
    msg: &Message,
    db: &Arc<Mutex<NotifyDb>>,
    config: &Arc<NotificationConfig>,
    debug: bool,
) -> Result<()> {
    // Verify this is a Notify method call
    let header = msg.header();
    let member = header.member();
    let interface = header.interface();

    if member.as_ref().map(|m| m.as_str()) != Some("Notify")
        || interface.as_ref().map(|i| i.as_str()) != Some("org.freedesktop.Notifications")
    {
        return Ok(());
    }

    // Parse the Notify arguments:
    // (app_name: str, replaces_id: u32, app_icon: str, summary: str, body: str,
    //  actions: array, hints: dict, timeout: i32)
    let body = msg.body();
    let fields: zbus::zvariant::Structure = body
        .deserialize()
        .context("deserializing Notify message body")?;

    let fields = fields.fields();

    if fields.len() < 5 {
        return Ok(());
    }

    let app_name: String = fields[0]
        .downcast_ref::<zbus::zvariant::Str>()
        .map(|s| s.as_str().to_string())
        .unwrap_or_default();

    let summary: String = fields[3]
        .downcast_ref::<zbus::zvariant::Str>()
        .map(|s| s.as_str().to_string())
        .unwrap_or_default();

    let body_text: String = fields[4]
        .downcast_ref::<zbus::zvariant::Str>()
        .map(|s| s.as_str().to_string())
        .unwrap_or_default();

    // Check ignore list
    if config.is_ignored(&app_name) {
        if debug {
            emit(
                Level::Debug,
                "notify.daemon.ignored",
                &format!("Ignored notification from {app_name}"),
                None,
            );
        }
        return Ok(());
    }

    // Truncate very long bodies (legacy behavior)
    let body_truncated: String = body_text.chars().take(500).collect();

    // Get current time
    let timestamp = Local::now().format("%H:%M").to_string();

    // Store in database
    let db = db.lock().await;
    db.add(&timestamp, &app_name, &summary, &body_truncated)?;

    // Play sound if not silenced
    if !config.is_silenced(&app_name) {
        // Sound playback is handled separately to avoid blocking
        let sound_path = config.notification_sound_path();
        if sound_path.exists() {
            // Spawn a non-blocking task for sound playback
            tokio::spawn(async move {
                let _ = duct::cmd!("mpv", "--keep-open=no", &sound_path).run();
            });
        }
    }

    if debug {
        emit(
            Level::Debug,
            "notify.daemon.captured",
            &format!("Captured: [{app_name}] {summary}: {body_truncated}"),
            None,
        );
    }

    Ok(())
}

/// Notification daemon configuration (ignore/silent lists).
struct NotificationConfig {
    ignore_apps: HashSet<String>,
    silent_apps: HashSet<String>,
    config_dir: PathBuf,
}

impl NotificationConfig {
    /// Load configuration from the instantOS config directory.
    fn load() -> Result<Self> {
        let config_dir = crate::common::paths::instant_config_dir()?;
        let ignore_path = config_dir.join("notifyignore");
        let silent_path = config_dir.join("notifysilent");

        let ignore_apps = load_app_list(&ignore_path);
        let silent_apps = load_app_list(&silent_path);

        Ok(Self {
            ignore_apps,
            silent_apps,
            config_dir,
        })
    }

    fn is_ignored(&self, app_name: &str) -> bool {
        self.ignore_apps.iter().any(|a| a.eq_ignore_ascii_case(app_name))
    }

    fn is_silenced(&self, app_name: &str) -> bool {
        self.silent_apps.iter().any(|a| a.eq_ignore_ascii_case(app_name))
    }

    /// Path to the notification sound file.
    fn notification_sound_path(&self) -> PathBuf {
        // Check for custom sound first
        let custom = self.config_dir.join("notifications").join("customsound");
        if custom.exists() {
            return custom;
        }
        // Default sound
        crate::common::paths::instant_data_dir()
            .unwrap_or_else(|_| PathBuf::from("~/.local/share/instant"))
            .join("notifications")
            .join("notification.ogg")
    }
}

/// Load a list of app names from a file (one per line).
fn load_app_list(path: &std::path::Path) -> HashSet<String> {
    let mut apps = HashSet::new();
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                apps.insert(trimmed.to_string());
            }
        }
    }
    apps
}
