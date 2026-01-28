//! Toggle settings for various desktop features
//!
//! Clipboard manager, auto-mount, and Bluetooth settings.

use anyhow::Result;
use std::fs;
use std::path::Path;
use std::process::Command;
use which::which;

use crate::common::package::{InstallResult, ensure_all};
use crate::common::systemd::{SystemdManager, UserServiceConfig};
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::deps::{BLUEZ, BLUEZ_UTILS, CLIPMENU, UDISKIE};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
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
            .requirements(vec![&CLIPMENU])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        // We don't store state in TOML anymore, we derive it from systemd
        SettingType::Action
    }

    fn get_display_state(&self, _ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        use crate::settings::setting::SettingState;

        // Check if package is installed first
        if !CLIPMENU.is_installed() {
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
            match CLIPMENU.ensure()? {
                InstallResult::Installed | InstallResult::AlreadyInstalled => {}
                _ => {
                    ctx.emit_info(
                        "settings.clipboard.aborted",
                        "Clipboard history setup was cancelled.",
                    );
                    return Ok(());
                }
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
        match UDISKIE.ensure()? {
            InstallResult::Installed | InstallResult::AlreadyInstalled => {}
            _ => {
                ctx.set_bool(Self::KEY, false);
                ctx.emit_info(
                    "settings.storage.udiskie.aborted",
                    "Auto-mount setup was cancelled.",
                );
                return Ok(());
            }
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
// Bluetooth Capability Check
// ============================================================================

struct BluetoothCapabilityDetails {
    sysfs_controllers: Vec<String>,
    bluetoothctl_controllers: Vec<String>,
    rfkill_entries: Vec<String>,
    lsusb_entries: Vec<String>,
}

impl BluetoothCapabilityDetails {
    fn detect() -> Self {
        Self {
            sysfs_controllers: detect_sysfs_controllers(),
            bluetoothctl_controllers: detect_bluetoothctl_controllers(),
            rfkill_entries: detect_rfkill_entries(),
            lsusb_entries: detect_lsusb_entries(),
        }
    }

    fn is_capable(&self) -> bool {
        !(self.sysfs_controllers.is_empty()
            && self.bluetoothctl_controllers.is_empty()
            && self.rfkill_entries.is_empty()
            && self.lsusb_entries.is_empty())
    }

    fn format_message(&self) -> String {
        let mut lines = Vec::new();
        let status = if self.is_capable() {
            "detected"
        } else {
            "not detected"
        };
        lines.push(format!("Bluetooth capability: {status}"));

        let mut has_details = false;

        if !self.sysfs_controllers.is_empty() {
            lines.push(String::new());
            lines.push("Controllers (sysfs):".to_string());
            lines.extend(
                self.sysfs_controllers
                    .iter()
                    .map(|item| format!("- {item}")),
            );
            has_details = true;
        }

        if !self.bluetoothctl_controllers.is_empty() {
            lines.push(String::new());
            lines.push("Controllers (bluetoothctl):".to_string());
            lines.extend(
                self.bluetoothctl_controllers
                    .iter()
                    .map(|item| format!("- {item}")),
            );
            has_details = true;
        }

        if !self.rfkill_entries.is_empty() {
            lines.push(String::new());
            lines.push("RFKill devices:".to_string());
            lines.extend(self.rfkill_entries.iter().map(|item| format!("- {item}")));
            has_details = true;
        }

        if !self.lsusb_entries.is_empty() {
            lines.push(String::new());
            lines.push("USB devices (lsusb):".to_string());
            lines.extend(self.lsusb_entries.iter().map(|item| format!("- {item}")));
            has_details = true;
        }

        if !has_details {
            lines.push(String::new());
            lines.push(
                "No controllers detected via sysfs, bluetoothctl, rfkill, or lsusb.".to_string(),
            );
            lines.push(
                "Tip: If you use a USB adapter, plug it in and re-run this check.".to_string(),
            );
        }

        lines.join("\n")
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn read_link_name(path: &Path) -> Option<String> {
    fs::read_link(path).ok().and_then(|link| {
        link.file_name()
            .map(|name| name.to_string_lossy().to_string())
    })
}

fn detect_sysfs_controllers() -> Vec<String> {
    let mut controllers = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/bluetooth") else {
        return controllers;
    };

    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("hci") {
            continue;
        }

        let path = entry.path();
        let mut details = Vec::new();

        if let Some(address) = read_trimmed(&path.join("address")) {
            details.push(format!("address {address}"));
        }
        if let Some(bus) = read_link_name(&path.join("device/subsystem")) {
            details.push(format!("bus {bus}"));
        }
        if let Some(driver) = read_link_name(&path.join("device/driver")) {
            details.push(format!("driver {driver}"));
        }
        if let Some(vendor) = read_trimmed(&path.join("device/idVendor"))
            .or_else(|| read_trimmed(&path.join("device/vendor")))
        {
            details.push(format!("vendor {vendor}"));
        }
        if let Some(product) = read_trimmed(&path.join("device/idProduct"))
            .or_else(|| read_trimmed(&path.join("device/device")))
        {
            details.push(format!("product {product}"));
        }
        if let Some(hci_type) = read_trimmed(&path.join("type")) {
            details.push(format!("type {hci_type}"));
        }

        if details.is_empty() {
            controllers.push(name);
        } else {
            controllers.push(format!("{name} ({})", details.join(", ")));
        }
    }

    controllers
}

fn detect_bluetoothctl_controllers() -> Vec<String> {
    if which("bluetoothctl").is_err() {
        return Vec::new();
    }

    let Ok(output) = Command::new("bluetoothctl").args(["list"]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("Controller ")?;
            let mut parts = rest.splitn(2, ' ');
            let address = parts.next()?.trim();
            let name = parts.next().unwrap_or("").trim();
            if name.is_empty() {
                Some(address.to_string())
            } else {
                Some(format!("{address} ({name})"))
            }
        })
        .collect()
}

fn detect_rfkill_entries() -> Vec<String> {
    if which("rfkill").is_err() {
        return Vec::new();
    }

    let Ok(output) = Command::new("rfkill").args(["list"]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    let mut header: Option<String> = None;
    let mut soft_blocked: Option<String> = None;
    let mut hard_blocked: Option<String> = None;

    let flush = |entries: &mut Vec<String>,
                 header: Option<String>,
                 soft: Option<String>,
                 hard: Option<String>| {
        let Some(header) = header else {
            return;
        };
        if !header.to_lowercase().contains("bluetooth") {
            return;
        }

        let mut summary = header;
        let mut details = Vec::new();
        if let Some(value) = soft {
            details.push(format!("soft blocked: {value}"));
        }
        if let Some(value) = hard {
            details.push(format!("hard blocked: {value}"));
        }
        if !details.is_empty() {
            summary.push_str(&format!(" ({})", details.join(", ")));
        }
        entries.push(summary);
    };

    for line in stdout.lines() {
        let trimmed = line.trim();
        let is_header = !line.starts_with(' ') && !line.starts_with('\t') && line.contains(": ");
        if is_header {
            flush(
                &mut entries,
                header.take(),
                soft_blocked.take(),
                hard_blocked.take(),
            );
            header = Some(trimmed.to_string());
            continue;
        }

        if let Some(value) = trimmed.strip_prefix("Soft blocked:") {
            soft_blocked = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("Hard blocked:") {
            hard_blocked = Some(value.trim().to_string());
        }
    }

    flush(&mut entries, header, soft_blocked, hard_blocked);
    entries
}

fn detect_lsusb_entries() -> Vec<String> {
    if which("lsusb").is_err() {
        return Vec::new();
    }

    let Ok(output) = Command::new("lsusb").output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.to_lowercase().contains("bluetooth") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn detect_bluetooth_hardware() -> bool {
    BluetoothCapabilityDetails::detect().is_capable()
}

pub struct BluetoothCapabilityCheck;

impl Setting for BluetoothCapabilityCheck {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("bluetooth.capability")
            .title("Check Bluetooth Capability")
            .icon(NerdFont::Info)
            .summary("Detect available Bluetooth hardware and report controller details.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        let details = BluetoothCapabilityDetails::detect();
        let message = details.format_message();
        FzfWrapper::builder()
            .message(message)
            .title("Bluetooth Capability Check")
            .show_message()?;
        Ok(())
    }
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

        fn ensure_bluetooth_ready(ctx: &mut SettingsContext) -> Result<bool> {
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

            match ensure_all(&[&BLUEZ, &BLUEZ_UTILS])? {
                InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(true),
                _ => Ok(false),
            }
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

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::Bluetooth))
    }

    // No restore needed - systemd handles service persistence
}
