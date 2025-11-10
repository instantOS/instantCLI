use anyhow::{Context, Result, bail};
use duct::cmd;
use std::process::Command;

use crate::common::systemd::{SystemdManager, UserServiceConfig};
use crate::menu_utils::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::context::SettingsContext;
use super::registry::{
    BLUETOOTH_CORE_PACKAGES, BLUETOOTH_HARDWARE_OVERRIDE_KEY, BLUETOOTH_SERVICE_KEY,
    COCKPIT_PACKAGES, PACMAN_AUTOCLEAN_KEY, UDISKIE_AUTOMOUNT_KEY, UDISKIE_PACKAGE,
};
use super::sources;

const BLUETOOTH_SERVICE_NAME: &str = "bluetooth";
const UDISKIE_SERVICE_NAME: &str = "udiskie";
const COCKPIT_SOCKET_NAME: &str = "cockpit.socket";

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
                    char::from(NerdFont::Warning)
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
                    char::from(NerdFont::Warning)
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

pub fn apply_udiskie_automount(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    // Ensure udiskie is installed
    if !UDISKIE_PACKAGE.ensure()? {
        ctx.set_bool(UDISKIE_AUTOMOUNT_KEY, false);
        ctx.emit_info(
            "settings.storage.udiskie.aborted",
            "Auto-mount setup was cancelled.",
        );
        return Ok(());
    }

    let systemd_manager = SystemdManager::user();

    if enabled {
        // Create the udiskie service configuration
        let service_config = UserServiceConfig::new(
            UDISKIE_SERVICE_NAME,
            "udiskie removable media automounter",
            "/usr/bin/udiskie",
        );

        // Create the user service file
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

        // Enable and start the service
        if !systemd_manager.is_enabled(UDISKIE_SERVICE_NAME) {
            if let Err(err) = systemd_manager.enable_and_start(UDISKIE_SERVICE_NAME) {
                emit(
                    Level::Warn,
                    "settings.storage.udiskie.enable_failed",
                    &format!(
                        "{} Failed to enable udiskie service: {err}",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
                return Err(err);
            }
        } else if !systemd_manager.is_active(UDISKIE_SERVICE_NAME)
            && let Err(err) = systemd_manager.start(UDISKIE_SERVICE_NAME)
        {
            emit(
                Level::Warn,
                "settings.storage.udiskie.start_failed",
                &format!(
                    "{} Failed to start udiskie service: {err}",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            return Err(err);
        }

        ctx.notify(
            "Auto-mount",
            "udiskie service enabled - removable drives will auto-mount",
        );
    } else {
        // Disable and stop the service
        if (systemd_manager.is_enabled(UDISKIE_SERVICE_NAME)
            || systemd_manager.is_active(UDISKIE_SERVICE_NAME))
            && let Err(err) = systemd_manager.disable_and_stop(UDISKIE_SERVICE_NAME)
        {
            emit(
                Level::Warn,
                "settings.storage.udiskie.disable_failed",
                &format!(
                    "{} Failed to disable udiskie service: {err}",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            return Err(err);
        }

        ctx.notify("Auto-mount", "udiskie service disabled");
    }

    Ok(())
}

/// Launch Cockpit web-based system management interface
pub fn launch_cockpit(ctx: &mut SettingsContext) -> Result<()> {
    // Ensure required packages are installed
    if !ctx.ensure_packages(COCKPIT_PACKAGES.as_slice())? {
        return Ok(());
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
        .spawn()?;

    Ok(())
}

#[derive(Clone)]
struct TimezoneChoice {
    value: String,
    is_current: bool,
}

fn timezone_preview_command() -> String {
    r#"bash -c '
tz="$1"

if [ -z "$tz" ]; then
  exit 0
fi

printf "═══════════════════════════════════════════════════\n"
printf "Timezone: %s\n" "$tz"
printf "═══════════════════════════════════════════════════\n\n"

current_local=$(TZ="$tz" date +"%Y-%m-%d %H:%M:%S %Z")
day_line=$(TZ="$tz" date +"%A, %d %B %Y")
twelve_hour=$(TZ="$tz" date +"%I:%M %p")
twenty_four=$(TZ="$tz" date +"%H:%M")
local_system=$(date +"%Y-%m-%d %H:%M:%S %Z")

printf "Current time:\n  %s\n  %s\n\n" "$current_local" "$day_line"

offset=$(TZ="$tz" date +%z)
sign=${offset:0:1}
hours=${offset:1:2}
mins=${offset:3:2}

printf "UTC offset:\n  UTC%s%s:%s\n\n" "$sign" "$hours" "$mins"

printf "12-hour clock:\n  %s\n" "$twelve_hour"
printf "24-hour clock:\n  %s\n\n" "$twenty_four"

printf "Local system time:\n  %s\n" "$local_system"
'"#
    .to_string()
}

impl FzfSelectable for TimezoneChoice {
    fn fzf_display_text(&self) -> String {
        let marker = if self.is_current {
            format!("{} ", char::from(NerdFont::Check))
        } else {
            "   ".to_string()
        };
        format!("{marker}{}", self.value)
    }

    fn fzf_key(&self) -> String {
        self.value.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Command(timezone_preview_command())
    }
}

fn read_command_lines(mut command: Command) -> Result<Vec<String>> {
    let program = command.get_program().to_owned();
    let output = command
        .output()
        .with_context(|| format!("running {:?}", program))?;

    if !output.status.success() {
        bail!(
            "command {:?} exited with status {:?}",
            program,
            output.status.code()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

fn current_timezone() -> Result<Option<String>> {
    let output = Command::new("timedatectl")
        .args(["show", "--property=Timezone", "--value"])
        .output()
        .context("running timedatectl show")?;

    if !output.status.success() {
        return Ok(None);
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(|s| s.to_string()))
}

pub fn configure_timezone(ctx: &mut SettingsContext) -> Result<()> {
    let timezones = read_command_lines({
        let mut command = Command::new("timedatectl");
        command.arg("list-timezones");
        command
    })?;

    if timezones.is_empty() {
        ctx.emit_info(
            "settings.timezone.none",
            "No timezones detected. Ensure tzdata is installed.",
        );
        return Ok(());
    }

    let current = current_timezone()?.unwrap_or_default();

    let choices: Vec<TimezoneChoice> = timezones
        .into_iter()
        .map(|value| TimezoneChoice {
            is_current: value == current,
            value,
        })
        .collect();

    let initial_index = choices.iter().position(|choice| choice.is_current);

    let mut builder = FzfWrapper::builder()
        .prompt("Timezone")
        .header("Select a timezone (preview shows current time)");

    if let Some(index) = initial_index {
        builder = builder.initial_index(index);
    }

    builder = builder.args(["--preview-window=right:50%:wrap"]);

    match builder.select(choices)? {
        FzfResult::Selected(choice) => {
            if choice.value == current {
                ctx.emit_info(
                    "settings.timezone.unchanged",
                    "Timezone already set to the selected value.",
                );
                return Ok(());
            }

            ctx.run_command_as_root("timedatectl", ["set-timezone", choice.value.as_str()])?;
            ctx.emit_success(
                "settings.timezone.updated",
                &format!("Timezone set to {}.", choice.value),
            );
            ctx.notify("Timezone", "System clock updated to the selected timezone.");
        }
        FzfResult::Error(err) => {
            bail!("fzf error: {err}");
        }
        _ => {}
    }

    Ok(())
}

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
