use anyhow::{Context, Result};
use duct::cmd;

use crate::fzf_wrapper::{ConfirmResult, FzfWrapper};
use crate::ui::prelude::*;

use super::context::SettingsContext;
use super::registry::{
    BLUETOOTH_CORE_PACKAGES, BLUETOOTH_HARDWARE_OVERRIDE_KEY, BLUETOOTH_SERVICE_KEY,
};

const BLUETOOTH_SERVICE_NAME: &str = "bluetooth";

pub fn apply_clipboard_manager(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let is_running = std::process::Command::new("pgrep")
        .arg("-f")
        .arg("clipmenud")
        .output()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false);

    if enabled && !is_running {
        if let Err(err) = std::process::Command::new("clipmenud").spawn() {
            emit(
                Level::Warn,
                "settings.clipboard.spawn_failed",
                &format!(
                    "{} Failed to launch clipmenud: {err}",
                    char::from(Fa::ExclamationCircle)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud started");
        }
    } else if !enabled && is_running {
        if let Err(err) = cmd!("pkill", "-f", "clipmenud").run() {
            emit(
                Level::Warn,
                "settings.clipboard.stop_failed",
                &format!(
                    "{} Failed to stop clipmenud: {err}",
                    char::from(Fa::ExclamationCircle)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud stopped");
        }
    }

    Ok(())
}

fn detect_bluetooth_hardware() -> bool {
    if let Ok(entries) = std::fs::read_dir("/sys/class/bluetooth")
        && entries.filter_map(Result::ok).next().is_some()
    {
        return true;
    }

    if let Ok(output) = std::process::Command::new("lsusb").output()
        && output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains("bluetooth")
    {
        return true;
    }

    if let Ok(output) = std::process::Command::new("rfkill").arg("list").output()
        && output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains("bluetooth")
    {
        return true;
    }

    false
}

fn service_check(args: &[&str]) -> bool {
    std::process::Command::new("systemctl")
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn service_is_active(service: &str) -> bool {
    service_check(&["is-active", "--quiet", service])
}

fn service_is_enabled(service: &str) -> bool {
    service_check(&["is-enabled", "--quiet", service])
}

fn ensure_bluetooth_ready(ctx: &mut SettingsContext) -> Result<bool> {
    if !ctx.bool(BLUETOOTH_HARDWARE_OVERRIDE_KEY) && !detect_bluetooth_hardware() {
        let result = FzfWrapper::builder()
            .confirm("System does not appear to have Bluetooth hardware. Proceed anyway?")
            .yes_text("Proceed")
            .no_text("Cancel")
            .show_confirmation()?;

        match result {
            ConfirmResult::Yes => {
                ctx.set_bool(BLUETOOTH_HARDWARE_OVERRIDE_KEY, true);
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                return Ok(false);
            }
        }
    }

    if !ctx.ensure_packages(BLUETOOTH_CORE_PACKAGES.as_slice())? {
        return Ok(false);
    }

    Ok(true)
}

pub fn apply_bluetooth_service(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    if enabled {
        if !ensure_bluetooth_ready(ctx)? {
            ctx.set_bool(BLUETOOTH_SERVICE_KEY, false);
            ctx.emit_info(
                "settings.bluetooth.service.aborted",
                "Bluetooth service enablement was cancelled.",
            );
            return Ok(());
        }

        if !service_is_enabled(BLUETOOTH_SERVICE_NAME) {
            ctx.run_command_as_root("systemctl", ["enable", "--now", BLUETOOTH_SERVICE_NAME])?;
        } else if !service_is_active(BLUETOOTH_SERVICE_NAME) {
            ctx.run_command_as_root("systemctl", ["start", BLUETOOTH_SERVICE_NAME])?;
        }

        ctx.notify("Bluetooth service", "Bluetooth service enabled");
    } else {
        if service_is_enabled(BLUETOOTH_SERVICE_NAME) || service_is_active(BLUETOOTH_SERVICE_NAME) {
            ctx.run_command_as_root("systemctl", ["disable", "--now", BLUETOOTH_SERVICE_NAME])?;
            ctx.notify("Bluetooth service", "Bluetooth service disabled");
        }
    }

    Ok(())
}
