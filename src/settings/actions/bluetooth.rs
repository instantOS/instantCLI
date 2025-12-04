//! Bluetooth-related settings actions

use anyhow::Result;

use crate::common::systemd::SystemdManager;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::prelude::*;

use super::super::context::SettingsContext;
use super::super::registry::{
    BLUETOOTH_CORE_PACKAGES, BLUETOOTH_HARDWARE_OVERRIDE_KEY, BLUETOOTH_SERVICE_KEY,
};

const BLUETOOTH_SERVICE_NAME: &str = "bluetooth";

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
    // Create a systemd manager with sudo support for system services
    let systemd = SystemdManager::system_with_sudo();

    if enabled {
        if !ensure_bluetooth_ready(ctx)? {
            ctx.set_bool(BLUETOOTH_SERVICE_KEY, false);
            ctx.emit_info(
                "settings.bluetooth.service.aborted",
                "Bluetooth service enablement was cancelled.",
            );
            return Ok(());
        }

        if !systemd.is_enabled(BLUETOOTH_SERVICE_NAME) {
            systemd.enable_and_start(BLUETOOTH_SERVICE_NAME)?;
        } else if !systemd.is_active(BLUETOOTH_SERVICE_NAME) {
            systemd.start(BLUETOOTH_SERVICE_NAME)?;
        }

        ctx.notify("Bluetooth service", "Bluetooth service enabled");
    } else if systemd.is_enabled(BLUETOOTH_SERVICE_NAME)
        || systemd.is_active(BLUETOOTH_SERVICE_NAME)
    {
        systemd.disable_and_stop(BLUETOOTH_SERVICE_NAME)?;
        ctx.notify("Bluetooth service", "Bluetooth service disabled");
    }

    Ok(())
}
