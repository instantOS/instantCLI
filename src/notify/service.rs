//! Lifecycle management for supervised notification capture.

use std::process::Command;

use anyhow::{Context, Result};

use crate::ui::prelude::*;

const SERVICE: &str = "ins-notify.service";
const BUS_NAME: &str = "org.instantos.NotificationHistory";

/// Return whether a capture daemon owns its well-known session-bus name.
pub async fn daemon_running() -> Result<bool> {
    let connection = zbus::Connection::session()
        .await
        .context("connecting to the session D-Bus")?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(BUS_NAME,),
        )
        .await
        .context("checking the notification capture daemon")?;
    reply
        .body()
        .deserialize()
        .context("reading notification capture status")
}

/// Enable the packaged user service and start it immediately.
pub fn enable_and_start() -> Result<()> {
    run_systemctl(&["enable", "--now", SERVICE])?;
    emit(
        Level::Success,
        "notify.service.enabled",
        "Notification capture enabled and started.",
        None,
    );
    Ok(())
}

/// Stop the packaged user service and disable future automatic starts.
pub fn disable_and_stop() -> Result<()> {
    run_systemctl(&["disable", "--now", SERVICE])?;
    emit(
        Level::Success,
        "notify.service.disabled",
        "Notification capture stopped and disabled.",
        None,
    );
    Ok(())
}

/// Print both the actual daemon state and systemd enablement state.
pub async fn show_status() -> Result<()> {
    let running = daemon_running().await.unwrap_or(false);
    let enabled = systemctl_succeeds(&["is-enabled", "--quiet", SERVICE])?;

    match get_output_format() {
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({ "running": running, "enabled": enabled })
        ),
        OutputFormat::Text => {
            println!(
                "Capture daemon: {}",
                if running { "running" } else { "stopped" }
            );
            println!(
                "Autostart: {}",
                if enabled { "enabled" } else { "disabled" }
            );
        }
    }
    Ok(())
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("running systemctl --user")?;
    anyhow::ensure!(
        status.success(),
        "systemctl could not manage {SERVICE}; packaged installs provide this user service. \
         For a standalone binary, run `ins notify daemon` from your session startup instead"
    );
    Ok(())
}

fn systemctl_succeeds(args: &[&str]) -> Result<bool> {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("running systemctl --user")
        .map(|status| status.success())
}
