//! System-related settings actions
//!
//! Handles timezone, pacman cache cleaning, and cockpit launcher.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::common::systemd::SystemdManager;
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::super::context::SettingsContext;
use super::super::registry::{COCKPIT_PACKAGES, PACMAN_AUTOCLEAN_KEY};
use super::super::sources;

const COCKPIT_SOCKET_NAME: &str = "cockpit.socket";

// --- Timezone ---

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

// --- Pacman autoclean ---

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

// --- Cockpit ---

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
