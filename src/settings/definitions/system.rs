//! System settings
//!
//! System administration, updates, and firmware settings.

use anyhow::{Context, Result};
use duct::cmd;

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
use dialoguer::console::Term;

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
// System Doctor (runs ins doctor and shows interactive fix menu)
// ============================================================================

pub struct SystemDoctor;

fn press_any_key_to_continue() -> Result<()> {
    println!();
    println!("Press any key to continue...");
    Term::stdout().read_key().context("waiting for key press")?;
    Ok(())
}

impl Setting for SystemDoctor {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.doctor")
            .title("System Diagnostics")
            .icon(NerdFont::ShieldCheck)
            .summary("Run system diagnostics to check for common issues and available fixes.")
            .requirements(vec![&PACMAN_CONTRIB])
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

        use crate::settings::doctor_integration;

        // Step 1: Display the full doctor results table and wait for user
        cmd!(env!("CARGO_BIN_NAME"), "doctor")
            .run()
            .context("running system doctor")?;
        press_any_key_to_continue()?;

        // Step 2: Get fixable issues from doctor JSON output
        let fixable_issues =
            doctor_integration::run_doctor_checks().context("getting fixable issues")?;

        // Step 3: Show fix menu if there are fixable issues
        if !fixable_issues.is_empty() {
            println!(); // Add spacing before menu

            let selected =
                doctor_integration::show_fix_menu(fixable_issues).context("showing fix menu")?;

            // Step 4: Execute selected fixes via CLI
            if !selected.is_empty() {
                doctor_integration::execute_fixes(selected).context("executing fixes")?;
                press_any_key_to_continue()?;

                ctx.notify(
                    "System Diagnostics",
                    "Fixes applied. Run diagnostics again to verify.",
                );
            }
        } else {
            ctx.notify("System Diagnostics", "No fixable issues found.");
        }

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
    &TOPGRADE
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
            .show_message()?;
    }

    // Launch chromium in app mode
    std::process::Command::new("chromium")
        .arg("--app=http://localhost:9090")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(())
}
