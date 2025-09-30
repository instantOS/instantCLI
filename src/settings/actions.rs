use anyhow::{Context, Result};
use duct::cmd;

use crate::fzf_wrapper::{ConfirmResult, FzfWrapper};
use crate::ui::prelude::*;

use super::context::SettingsContext;
use super::registry::{
    BLUETOOTH_APPLET_KEY, BLUETOOTH_APPLET_PACKAGES, BLUETOOTH_CORE_PACKAGES,
    BLUETOOTH_HARDWARE_OVERRIDE_KEY, BLUETOOTH_SERVICE_KEY,
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
    if let Ok(entries) = std::fs::read_dir("/sys/class/bluetooth") {
        if entries.filter_map(Result::ok).next().is_some() {
            return true;
        }
    }

    if let Ok(output) = std::process::Command::new("lsusb").output() {
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .to_lowercase()
                .contains("bluetooth")
        {
            return true;
        }
    }

    if let Ok(output) = std::process::Command::new("rfkill").arg("list").output() {
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .to_lowercase()
                .contains("bluetooth")
        {
            return true;
        }
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

fn ensure_bluetooth_ready(ctx: &mut SettingsContext, include_applet: bool) -> Result<bool> {
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

    let packages: &[crate::common::requirements::RequiredPackage] = if include_applet {
        BLUETOOTH_APPLET_PACKAGES.as_slice()
    } else {
        BLUETOOTH_CORE_PACKAGES.as_slice()
    };

    if !ctx.ensure_packages(packages)? {
        return Ok(false);
    }

    Ok(true)
}

fn blueman_running() -> bool {
    std::process::Command::new("pgrep")
        .arg("-f")
        .arg("blueman-applet")
        .output()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false)
}

fn stop_blueman_applet(ctx: &SettingsContext) {
    if let Err(err) = cmd!("pkill", "-f", "blueman-applet").run() {
        emit(
            Level::Warn,
            "settings.bluetooth.applet.stop_failed",
            &format!(
                "{} Failed to stop blueman-applet: {err}",
                char::from(Fa::ExclamationCircle)
            ),
            None,
        );
    } else {
        ctx.notify("Bluetooth applet", "Blueman applet stopped");
    }
}

pub fn apply_bluetooth_service(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    if enabled {
        if !ensure_bluetooth_ready(ctx, false)? {
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

        if ctx.bool(BLUETOOTH_APPLET_KEY) {
            stop_blueman_applet(ctx);
            ctx.set_bool(BLUETOOTH_APPLET_KEY, false);
            ctx.emit_info(
                "settings.bluetooth.applet.disabled",
                "Bluetooth applet was stopped because the service is disabled.",
            );
        }
    }

    Ok(())
}

pub fn apply_bluetooth_applet(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    if enabled {
        if !ensure_bluetooth_ready(ctx, true)? {
            ctx.set_bool(BLUETOOTH_APPLET_KEY, false);
            ctx.emit_info(
                "settings.bluetooth.applet.aborted",
                "Bluetooth applet launch was cancelled.",
            );
            return Ok(());
        }

        if !service_is_active(BLUETOOTH_SERVICE_NAME) {
            ctx.run_command_as_root("systemctl", ["start", BLUETOOTH_SERVICE_NAME])?;
        }

        if !blueman_running() {
            std::process::Command::new("blueman-applet")
                .spawn()
                .with_context(|| "spawning blueman-applet")?;
            ctx.notify("Bluetooth applet", "Blueman applet started");
        }
    } else if blueman_running() {
        stop_blueman_applet(ctx);
    }

    Ok(())
}
