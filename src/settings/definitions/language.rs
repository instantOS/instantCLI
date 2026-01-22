//! Language & region settings
//!
//! System language/locale configuration and timezone.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// System Language
// ============================================================================

pub struct SystemLanguage;

impl Setting for SystemLanguage {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.main")
            .title("Language")
            .icon(NerdFont::Globe)
            .summary("Manage system locales and choose the default language.\n\nEnable or disable locales in /etc/locale.gen and set LANG via localectl.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        crate::settings::language::configure_system_language(ctx)
    }
}

// ============================================================================
// Timezone
// ============================================================================

pub struct Timezone;

impl Setting for Timezone {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.timezone")
            .title("Timezone")
            .icon(NerdFont::Clock)
            .summary("Select the system timezone via timedatectl set-timezone.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        configure_timezone(ctx)
    }
}

// ============================================================================
// Timezone Implementation
// ============================================================================

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
    // Check for systemd availability (timedatectl)
    if which::which("timedatectl").is_err() {
        ctx.emit_unsupported(
            "settings.timezone.no_systemd",
            "Timezone configuration requires systemd (timedatectl not found).",
        );
        return Ok(());
    }

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
