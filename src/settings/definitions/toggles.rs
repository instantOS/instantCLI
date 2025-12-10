//! Toggle settings for various desktop features
//!
//! Clipboard manager, auto-mount, and Bluetooth settings.

use anyhow::Result;

use crate::common::requirements::UDISKIE_PACKAGE;
use crate::common::systemd::{SystemdManager, UserServiceConfig};
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Requirement, Setting, SettingMetadata, SettingType};
use crate::settings::store::BoolSettingKey;
use crate::ui::prelude::*;

// ============================================================================
// Clipboard Manager
// ============================================================================

// ============================================================================
// Clipboard Manager
// ============================================================================

pub struct ClipboardManager;

impl Setting for ClipboardManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("desktop.clipboard")
            .title("Clipboard History")
            .icon(NerdFont::Clipboard)
            .summary("Remember your copy/paste history so you can access previously copied items.\n\nWhen enabled, you can paste from your clipboard history instead of just the last copied item.")
            .requirements(&[Requirement::Package(
                crate::common::requirements::CLIPMENU_PACKAGE,
            )])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        // We don't store state in TOML anymore, we derive it from systemd
        SettingType::Action
    }

    fn get_display_state(&self, _ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        use crate::settings::setting::SettingState;

        // Check if package is installed first
        if !crate::common::requirements::CLIPMENU_PACKAGE.is_installed() {
            return SettingState::Toggle { enabled: false };
        }

        // Check systemd service status
        let systemd = SystemdManager::user();
        let enabled = systemd.is_enabled("clipmenud") || systemd.is_active("clipmenud");

        SettingState::Toggle { enabled }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        use crate::settings::setting::SettingState;

        let current_state = self.get_display_state(ctx);
        let currently_enabled = match current_state {
            SettingState::Toggle { enabled } => enabled,
            _ => false,
        };

        // Toggle logic
        let should_enable = !currently_enabled;

        const CLIPMENU_SERVICE: &str = "clipmenud";

        if should_enable {
            // Ensure package is installed before trying to enable service
            if !crate::common::requirements::CLIPMENU_PACKAGE.ensure()? {
                ctx.emit_info(
                    "settings.clipboard.aborted",
                    "Clipboard history setup was cancelled.",
                );
                return Ok(());
            }

            let systemd = SystemdManager::user();
            if !systemd.is_enabled(CLIPMENU_SERVICE) {
                systemd.enable_and_start(CLIPMENU_SERVICE)?;
            } else if !systemd.is_active(CLIPMENU_SERVICE) {
                systemd.start(CLIPMENU_SERVICE)?;
            }

            ctx.notify("Clipboard manager", "Clipboard history enabled");
        } else {
            // Disable
            let systemd = SystemdManager::user();
            if systemd.is_enabled(CLIPMENU_SERVICE) || systemd.is_active(CLIPMENU_SERVICE) {
                systemd.disable_and_stop(CLIPMENU_SERVICE)?;
                ctx.notify("Clipboard manager", "Clipboard history disabled");
            }
        }

        Ok(())
    }
}

// ============================================================================
// Auto-mount Disks
// ============================================================================

pub struct AutomountDisks;

impl AutomountDisks {
    const KEY: BoolSettingKey = BoolSettingKey::new("storage.udiskie", false);
}

impl Setting for AutomountDisks {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("storage.automount")
            .title("Auto-mount disks")
            .icon(NerdFont::HardDrive)
            .summary("Automatically mount removable drives with udiskie.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);

        const UDISKIE_SERVICE_NAME: &str = "udiskie";

        // Ensure udiskie is installed
        if !UDISKIE_PACKAGE.ensure()? {
            ctx.set_bool(Self::KEY, false);
            ctx.emit_info(
                "settings.storage.udiskie.aborted",
                "Auto-mount setup was cancelled.",
            );
            return Ok(());
        }

        let systemd_manager = SystemdManager::user();

        if enabled {
            let service_config = UserServiceConfig::new(
                UDISKIE_SERVICE_NAME,
                "udiskie removable media automounter",
                "/usr/bin/udiskie",
            );

            if let Err(err) = systemd_manager.create_user_service(&service_config) {
                emit(
                    Level::Warn,
                    "settings.storage.udiskie.service_creation_failed",
                    &format!(
                        "{} Failed to create udiskie service file: {err}",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
                return Err(err);
            }

            if !systemd_manager.is_enabled(UDISKIE_SERVICE_NAME) {
                systemd_manager.enable_and_start(UDISKIE_SERVICE_NAME)?;
            } else if !systemd_manager.is_active(UDISKIE_SERVICE_NAME) {
                systemd_manager.start(UDISKIE_SERVICE_NAME)?;
            }

            ctx.notify(
                "Auto-mount",
                "udiskie service enabled - removable drives will auto-mount",
            );
        } else if systemd_manager.is_enabled(UDISKIE_SERVICE_NAME)
            || systemd_manager.is_active(UDISKIE_SERVICE_NAME)
        {
            systemd_manager.disable_and_stop(UDISKIE_SERVICE_NAME)?;
            ctx.notify("Auto-mount", "udiskie service disabled");
        }

        Ok(())
    }

    // No restore needed - systemd handles service persistence
}

// ============================================================================
// Bluetooth Service
// ============================================================================

pub struct BluetoothService;

impl BluetoothService {
    const KEY: BoolSettingKey = BoolSettingKey::new("bluetooth.service", false);
    const HARDWARE_OVERRIDE_KEY: BoolSettingKey =
        BoolSettingKey::new("bluetooth.hardware_override", false);
}

impl Setting for BluetoothService {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("bluetooth.service")
            .title("Enable Bluetooth")
            .icon(NerdFont::Bluetooth)
            .summary("Turn Bluetooth on or off.\n\nWhen enabled, you can connect wireless devices like headphones, keyboards, and mice.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);

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
            use crate::common::requirements::{BLUEZ_PACKAGE, BLUEZ_UTILS_PACKAGE};

            if !ctx.bool(BluetoothService::HARDWARE_OVERRIDE_KEY) && !detect_bluetooth_hardware() {
                let result = FzfWrapper::builder()
                    .confirm("System does not appear to have Bluetooth hardware. Proceed anyway?")
                    .yes_text("Proceed")
                    .no_text("Cancel")
                    .show_confirmation()?;

                match result {
                    ConfirmResult::Yes => {
                        ctx.set_bool(BluetoothService::HARDWARE_OVERRIDE_KEY, true);
                    }
                    ConfirmResult::No | ConfirmResult::Cancelled => {
                        return Ok(false);
                    }
                }
            }

            if !ctx.ensure_packages(&[BLUEZ_PACKAGE, BLUEZ_UTILS_PACKAGE])? {
                return Ok(false);
            }

            Ok(true)
        }

        let systemd = SystemdManager::system_with_sudo();

        if enabled {
            if !ensure_bluetooth_ready(ctx)? {
                ctx.set_bool(Self::KEY, false);
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

    // No restore needed - systemd handles service persistence
}
