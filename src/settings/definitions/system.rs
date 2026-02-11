//! System settings
//!
//! System administration, updates, and firmware settings.

use anyhow::{Context, Result};
use duct::cmd;

use crate::arch::dualboot::types::format_size;
use crate::common::distro::OperatingSystem;
use crate::common::package::{InstallResult, ensure_all};
use crate::common::systemd::SystemdManager;
use crate::menu_utils::FzfWrapper;
use crate::settings::context::SettingsContext;
use crate::settings::deps::{
    COCKPIT, COCKPIT_DEPS, FASTFETCH, GNOME_FIRMWARE, PACMAN_CONTRIB, TOPGRADE,
};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::sources;
use crate::settings::store::{BoolSettingKey, PACMAN_AUTOCLEAN_KEY};
use crate::ui::prelude::*;

// ============================================================================
// About System (uses shell command with read, can't use macro)
// ============================================================================

pub struct AboutSystem;

impl Setting for AboutSystem {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.about")
            .title("About")
            .icon(NerdFont::About)
            .summary("Display system information using fastfetch.")
            .requirements(vec![&FASTFETCH])
            .search_keywords(&["fastfetch", "neofetch"])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info(
            "settings.command.launching",
            "Displaying system information...",
        );
        cmd!("sh", "-c", "fastfetch && read -n 1")
            .run()
            .context("running fastfetch")?;
        Ok(())
    }
}

// ============================================================================
// System Doctor (runs ins doctor fix --choose for interactive diagnostics)
// ============================================================================

pub struct SystemDoctor;

impl Setting for SystemDoctor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.doctor")
            .title("System Diagnostics")
            .icon(NerdFont::ShieldCheck)
            .summary("Run system diagnostics to check for common issues and available fixes.")
            .requirements(vec![&PACMAN_CONTRIB])
            .search_keywords(&["health"])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info(
            "settings.command.launching",
            "Running system diagnostics...",
        );

        // Simply call the new doctor fix --choose command
        // It handles everything: diagnosis, interactive menu, and fixes
        cmd!(env!("CARGO_BIN_NAME"), "doctor", "fix", "--choose")
            .run()
            .context("running interactive doctor fix")?;

        ctx.notify("System Diagnostics", "Diagnostic session completed.");
        Ok(())
    }
}

// ============================================================================
// Dotfile Manager (runs ins dot menu for interactive dotfile management)
// ============================================================================

pub struct DotfileManager;

impl Setting for DotfileManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.dotfiles")
            .title("Dotfile Manager")
            .icon(NerdFont::File)
            .summary("Manage dotfile repositories, subdirectories, and file sources.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Opening dotfile manager...");

        cmd!(env!("CARGO_BIN_NAME"), "dot", "menu")
            .run()
            .context("running dotfile menu")?;

        Ok(())
    }
}

// ============================================================================
// Cockpit (uses custom launch logic, can't use macro)
// ============================================================================

pub struct CockpitManager;

impl Setting for CockpitManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.cockpit")
            .title("Systemd manager (Cockpit)")
            .icon(NerdFont::Server)
            .summary("Launch Cockpit web interface for managing systemd services, logs, and system resources.")
            .requirements(vec![&COCKPIT])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        launch_cockpit(ctx)
    }
}

// ============================================================================
// Firmware Manager (GUI app)
// ============================================================================

gui_command_setting!(
    FirmwareManager,
    "system.firmware",
    "Firmware Manager",
    NerdFont::Cpu,
    "Launch GNOME Firmware manager to view and update device firmware.\n\nManage firmware for BIOS/UEFI, devices, and peripherals.",
    "gnome-firmware",
    &GNOME_FIRMWARE
);

// ============================================================================
// System Upgrade (TUI app)
// ============================================================================

tui_command_setting!(
    SystemUpgrade,
    "system.upgrade",
    "Upgrade",
    NerdFont::Upgrade,
    "Upgrade all installed packages and system components using topgrade.",
    "topgrade",
    &TOPGRADE,
    &["update"]
);

// ============================================================================
// Pacman Cache Autoclean
// ============================================================================

pub struct PacmanAutoclean;

impl PacmanAutoclean {
    const KEY: BoolSettingKey = BoolSettingKey::new("system.pacman_autoclean", false);
}

impl Setting for PacmanAutoclean {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.pacman_autoclean")
            .title("Pacman cache autoclean")
            .icon(NerdFont::Trash)
            .summary("Run paccache weekly to keep only the latest pacman packages.")
            .requirements(vec![&PACMAN_CONTRIB])
            .supported_distros(&[OperatingSystem::Arch])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let target = !current;
        ctx.set_bool(Self::KEY, target);
        apply_pacman_autoclean(ctx, target)
    }
}

// ============================================================================
// Clear Pacman Cache
// ============================================================================

pub struct ClearPacmanCache;

impl Setting for ClearPacmanCache {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.pacman_cache_clear")
            .title("Clear pacman cache")
            .icon(NerdFont::Trash)
            .summary("Remove all cached pacman packages using pacman -Scc.")
            .supported_distros(&[OperatingSystem::Arch])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let cache_size = describe_pacman_cache_size();
        let message = format!("Clear pacman cache?\n\nCache size: {cache_size}");

        let result = FzfWrapper::builder()
            .confirm(message)
            .yes_text("Clear cache")
            .no_text("Keep cache")
            .confirm_dialog()?;

        if matches!(result, crate::menu_utils::ConfirmResult::Yes) {
            ctx.emit_info("settings.pacman_cache.clearing", "Clearing pacman cache...");
            ctx.run_command_as_root("pacman", ["-Scc"])?;
            ctx.notify("Pacman cache", "Pacman cache cleared.");
        }

        Ok(())
    }
}

// Note: PacmanAutoclean cannot use simple_toggle_setting! macro because it has
// custom apply logic (calls apply_pacman_autoclean) and requirements that need
// to be checked. The simple_toggle_setting! macro is only for toggles with no
// additional logic beyond flipping the value and showing a message.

// ============================================================================
// Welcome App Autostart
// ============================================================================

simple_toggle_setting!(
    WelcomeAutostart,
    "system.welcome_autostart",
    "Welcome app on startup",
    NerdFont::Home,
    "Show the welcome application automatically when logging in.\n\nThe welcome app provides quick access to the instantOS website and system settings.",
    true,
    "Welcome app will appear on next startup",
    "Welcome app autostart has been disabled"
);

// ============================================================================
// Implementations
// ============================================================================

pub fn apply_pacman_autoclean(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    if let Some(source) = sources::source_for(&PACMAN_AUTOCLEAN_KEY) {
        source.apply(enabled)?;
        let active = ctx.refresh_bool_source(PACMAN_AUTOCLEAN_KEY)?;

        if active {
            ctx.notify(
                "Pacman cache",
                "Automatic weekly pacman cache cleanup enabled.",
            );
        } else {
            ctx.notify("Pacman cache", "Automatic pacman cache cleanup disabled.");
        }
    } else {
        ctx.set_bool(PACMAN_AUTOCLEAN_KEY, enabled);
        ctx.notify(
            "Pacman cache",
            if enabled {
                "Automatic weekly pacman cache cleanup enabled."
            } else {
                "Automatic pacman cache cleanup disabled."
            },
        );
    }

    Ok(())
}

const PACMAN_CACHE_DIR: &str = "/var/cache/pacman/pkg";

fn describe_pacman_cache_size() -> String {
    match calculate_dir_size(PACMAN_CACHE_DIR) {
        Ok(size) => format_size(size),
        Err(_) => "Unknown".to_string(),
    }
}

fn calculate_dir_size(path: &str) -> Result<u64> {
    let cache_path = std::path::Path::new(path);
    if !cache_path.exists() {
        return Ok(0);
    }

    let mut total_size: u64 = 0;
    let mut dirs_to_visit = vec![cache_path.to_path_buf()];

    while let Some(dir) = dirs_to_visit.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };

            if metadata.is_file() {
                total_size += metadata.len();
            } else if metadata.is_dir() {
                dirs_to_visit.push(entry.path());
            }
        }
    }

    Ok(total_size)
}

const COCKPIT_SOCKET_NAME: &str = "cockpit.socket";

/// Launch Cockpit web-based system management interface
pub fn launch_cockpit(ctx: &mut SettingsContext) -> Result<()> {
    // Ensure required packages are installed
    match ensure_all(COCKPIT_DEPS)? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => {}
        _ => {
            ctx.emit_info("settings.cockpit.cancelled", "Cockpit launch cancelled.");
            return Ok(());
        }
    }

    let systemd = SystemdManager::system_with_sudo();

    // Check if cockpit.socket is enabled, if not enable it
    if !systemd.is_enabled(COCKPIT_SOCKET_NAME) {
        systemd.enable_and_start(COCKPIT_SOCKET_NAME)?;

        // Give cockpit a moment to start up
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Show login hint
        let username = std::env::var("USER").unwrap_or_else(|_| "your username".to_string());
        FzfWrapper::builder()
            .message(format!(
                "Cockpit is starting...\n\nSign in with '{}' in the browser window.",
                username
            ))
            .title("Cockpit")
            .message_dialog()?;
    }

    // Launch chromium in app mode
    std::process::Command::new("chromium")
        .arg("--app=http://localhost:9090")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(())
}
