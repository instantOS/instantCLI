use std::process::Command;

use anyhow::{Result, bail};
use duct::cmd;
use sudo::RunningAs;

use crate::common::requirements::RequiredPackage;
use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;

use super::sources;
use super::store::{
    BoolSettingKey, IntSettingKey, OptionalStringSettingKey, SettingsStore, StringSettingKey,
};

#[derive(Debug)]
pub struct SettingsContext {
    store: SettingsStore,
    dirty: bool,
    debug: bool,
    privileged: bool,
}

impl SettingsContext {
    pub fn new(store: SettingsStore, debug: bool, privileged_flag: bool) -> Self {
        let mut ctx = Self {
            store,
            dirty: false,
            debug,
            privileged: privileged_flag || matches!(sudo::check(), RunningAs::Root),
        };

        ctx.sync_external_states();
        ctx
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn is_privileged(&self) -> bool {
        self.privileged
    }

    pub fn bool(&self, key: BoolSettingKey) -> bool {
        if let Some(source) = sources::source_for(&key) {
            match source.current() {
                Ok(value) => value,
                Err(err) => {
                    emit(
                        Level::Warn,
                        "settings.state.read_failed",
                        &format!(
                            "{} Failed to read state for '{}': {err}",
                            char::from(NerdFont::Warning),
                            key.key
                        ),
                        None,
                    );
                    self.store.bool(key)
                }
            }
        } else {
            self.store.bool(key)
        }
    }

    pub fn set_bool(&mut self, key: BoolSettingKey, value: bool) {
        if self.store.bool(key) != value {
            self.store.set_bool(key, value);
            self.dirty = true;
        }
    }

    pub fn string(&self, key: StringSettingKey) -> String {
        self.store.string(key)
    }

    pub fn set_string(&mut self, key: StringSettingKey, value: &str) {
        if self.store.string(key) != value {
            self.store.set_string(key, value);
            self.dirty = true;
        }
    }

    pub fn int(&self, key: IntSettingKey) -> i64 {
        self.store.int(key)
    }

    pub fn set_int(&mut self, key: IntSettingKey, value: i64) {
        if self.store.int(key) != value {
            self.store.set_int(key, value);
            self.dirty = true;
        }
    }

    pub fn optional_string(&self, key: OptionalStringSettingKey) -> Option<String> {
        self.store.optional_string(key)
    }

    pub fn set_optional_string<S: Into<String>>(
        &mut self,
        key: OptionalStringSettingKey,
        value: Option<S>,
    ) {
        self.store.set_optional_string(key, value);
        self.dirty = true;
    }

    pub fn contains(&self, key: &str) -> bool {
        self.store.contains(key)
    }

    fn sync_external_states(&mut self) {
        for &(key_ref, source) in sources::all_bool_sources() {
            let key = *key_ref;
            if let Err(err) = self.update_bool_from_source(key, source) {
                emit(
                    Level::Warn,
                    "settings.state.sync_failed",
                    &format!(
                        "{} Failed to synchronize state for '{}': {err}",
                        char::from(NerdFont::Warning),
                        key.key
                    ),
                    None,
                );
            }
        }
    }

    fn update_bool_from_source(
        &mut self,
        key: BoolSettingKey,
        source: &'static dyn sources::BoolStateSource,
    ) -> Result<bool> {
        let current = source.current()?;
        if self.store.bool(key) != current {
            self.store.set_bool(key, current);
            self.dirty = true;
        }
        Ok(current)
    }

    pub fn refresh_bool_source(&mut self, key: BoolSettingKey) -> Result<bool> {
        if let Some(source) = sources::source_for(&key) {
            match self.update_bool_from_source(key, source) {
                Ok(value) => Ok(value),
                Err(err) => {
                    emit(
                        Level::Warn,
                        "settings.state.refresh_failed",
                        &format!(
                            "{} Failed to refresh state for '{}': {err}",
                            char::from(NerdFont::Warning),
                            key.key
                        ),
                        None,
                    );
                    Ok(self.store.bool(key))
                }
            }
        } else {
            Ok(self.store.bool(key))
        }
    }

    pub fn ensure_packages(&mut self, packages: &[RequiredPackage]) -> Result<bool> {
        crate::common::requirements::ensure_packages_batch(packages)
    }

    pub fn persist(&mut self) -> Result<()> {
        if self.dirty {
            self.store.save()?;
            self.dirty = false;
        }
        Ok(())
    }

    pub fn notify(&self, summary: &str, body: &str) {
        if self.debug {
            let message = format!("{} {summary}: {body}", char::from(NerdFont::Info));
            emit(Level::Debug, "settings.notify", &message, None);
            return;
        }

        let result = cmd!("notify-send", summary, body).run();
        if let Err(err) = result {
            let message = format!(
                "{} Failed to send notification: {err}",
                char::from(NerdFont::Warning)
            );
            emit(Level::Debug, "settings.notify.error", &message, None);
        }
    }

    pub fn show_message(&self, message: &str) {
        // Best-effort; user feedback in TUI context
        let _ = FzfWrapper::message_dialog(message);
    }

    pub fn emit_success(&self, code: &str, message: &str) {
        emit(
            Level::Success,
            code,
            &format!("{} {message}", char::from(NerdFont::Check)),
            None,
        );
    }

    pub fn emit_info(&self, code: &str, message: &str) {
        emit(
            Level::Info,
            code,
            &format!("{} {message}", char::from(NerdFont::Info)),
            None,
        );
    }

    pub fn emit_unsupported(&self, code: &str, message: &str) {
        self.emit_info(code, message);
        self.show_message(message);
    }

    pub fn emit_failure(&self, code: &str, message: &str) {
        emit(Level::Warn, code, message, None);
        self.show_message(message);
    }

    pub fn run_command_as_root<I, S>(&self, program: S, args: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: AsRef<std::ffi::OsStr>,
        S: AsRef<std::ffi::OsStr>,
    {
        let program_os = program.as_ref().to_owned();
        let status = if self.privileged {
            let mut command = Command::new(&program_os);
            command.args(args);
            command.status()
        } else {
            let mut command = Command::new("/usr/bin/sudo");
            command.arg(&program_os);
            command.args(args);
            command.status()
        }?;

        if !status.success() {
            bail!(
                "command {:?} failed with status {:?}",
                program_os,
                status.code()
            );
        }

        Ok(())
    }

    // fn invoke_privileged(&mut self, value: PrivilegedValue) -> Result<()> {
    //     if self.privileged {
    //         return Ok(());
    //     }
    //
    //     let definition = self.current_definition.ok_or_else(|| {
    //         anyhow::anyhow!("no active setting definition for privilege escalation")
    //     })?;
    //
    //     let exe = env::current_exe().context("locating executable")?;
    //     let settings_path = self.store.path().to_path_buf();
    //
    //     let mut command = Command::new("/usr/bin/sudo");
    //     command.arg(&exe);
    //     if self.debug {
    //         command.arg("--debug");
    //     }
    //     command.arg("--internal-privileged-mode");
    //     command.arg("settings");
    //     command.arg("internal-apply");
    //     command.arg("--setting-id");
    //     command.arg(definition.id);
    //     command.arg("--settings-file");
    //     command.arg(&settings_path);
    //     match value {
    //         PrivilegedValue::Bool(v) => {
    //             command.arg("--bool-value");
    //             command.arg(if v { "true" } else { "false" });
    //         }
    //         PrivilegedValue::Choice(v) => {
    //             command.arg("--string-value");
    //             command.arg(v);
    //         }
    //     }
    //
    //     let status = command
    //         .status()
    //         .with_context(|| format!("escalating setting {}", definition.id))?;
    //
    //     if !status.success() {
    //         bail!(
    //             "privileged apply for {} exited with status {:?}",
    //             definition.id,
    //             status.code()
    //         );
    //     }
    //
    //     Ok(())
    // }
}

/// Catppuccin Mocha color palette for ANSI output
pub mod colors {
    // Accent colors
    pub const ROSEWATER: &str = "#f5e0dc";
    pub const FLAMINGO: &str = "#f2cdcd";
    pub const PINK: &str = "#f5c2e7";
    pub const MAUVE: &str = "#cba6f7";
    pub const RED: &str = "#f38ba8";
    pub const MAROON: &str = "#eba0ac";
    pub const PEACH: &str = "#fab387";
    pub const YELLOW: &str = "#f9e2af";
    pub const GREEN: &str = "#a6e3a1";
    pub const TEAL: &str = "#94e2d5";
    pub const SKY: &str = "#89dceb";
    pub const SAPPHIRE: &str = "#74c7ec";
    pub const BLUE: &str = "#89b4fa";
    pub const LAVENDER: &str = "#b4befe";
    // Surface colors
    pub const SURFACE0: &str = "#313244";
    pub const SURFACE1: &str = "#45475a";
    pub const SURFACE2: &str = "#585b70";
    // Overlay colors
    pub const OVERLAY0: &str = "#6c7086";
    pub const OVERLAY1: &str = "#7f849c";
    pub const OVERLAY2: &str = "#9399b2";
    // Text colors
    pub const SUBTEXT0: &str = "#a6adc8";
    pub const SUBTEXT1: &str = "#bac2de";
    pub const TEXT: &str = "#cdd6f4";
    // Base colors (backgrounds)
    pub const BASE: &str = "#1e1e2e";
    pub const MANTLE: &str = "#181825";
    pub const CRUST: &str = "#11111b";
}

/// Format an icon with colored background badge (uses Catppuccin Blue by default)
pub fn format_icon(icon: NerdFont) -> String {
    format_icon_colored(icon, colors::BLUE)
}

/// Format an icon with a colored background badge (hex format like "#89b4fa")
/// Creates a pill-shaped badge with dark text on colored background.
/// Uses targeted ANSI reset (not \x1b[0m) to preserve FZF color compatibility.
pub fn format_icon_colored(icon: NerdFont, bg_color: &str) -> String {
    let bg = hex_to_ansi_bg(bg_color);
    let fg = hex_to_ansi_fg(colors::CRUST); // Dark text for badge
    // Reset background (49) and set foreground to match FZF's text color
    // Using \x1b[49m resets only background; \x1b[39m uses default foreground
    let reset = "\x1b[49;39m";
    // Padding inside the colored badge
    format!("{bg}{fg}   {}   {reset} ", char::from(icon))
}

/// Format the back button icon with a neutral color
pub fn format_back_icon() -> String {
    format_icon_colored(NerdFont::ArrowLeft, colors::OVERLAY1)
}

/// Format the search icon with its own color
pub fn format_search_icon() -> String {
    format_icon_colored(NerdFont::Search, colors::MAUVE)
}

/// Convert hex color to ANSI 24-bit true color foreground escape sequence
pub fn hex_to_ansi_fg(hex: &str) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return String::new();
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Convert hex color to ANSI 24-bit true color background escape sequence
fn hex_to_ansi_bg(hex: &str) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return String::new();
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    format!("\x1b[48;2;{r};{g};{b}m")
}

pub fn select_one_with_style_at<T>(items: Vec<T>, initial_index: Option<usize>) -> Result<Option<T>>
where
    T: crate::menu_utils::FzfSelectable + Clone,
{
    // Build styled fzf with modern Catppuccin Mocha theme
    let mut builder = crate::menu_utils::FzfWrapper::builder()
        .prompt(format!("{} ", char::from(NerdFont::Search)))
        .header(" ") // Add gap between prompt and list
        .args([
            // Visual styling
            "--no-separator",
            "--padding=1,2",
            "--list-border=none",
            "--input-border=none",
            "--preview-border=left",
            "--pointer=▌",
            // Catppuccin Mocha color scheme
            "--color=bg:#1e1e2e",         // Base - main background
            "--color=bg+:#313244",        // Surface0 - highlighted item bg
            "--color=fg:#cdd6f4",         // Text - normal foreground
            "--color=fg+:#cdd6f4",        // Text - highlighted foreground
            "--color=preview-bg:#181825", // Mantle - preview pane bg
            "--color=hl:#f9e2af",         // Yellow - matched text
            "--color=hl+:#f9e2af",        // Yellow - matched text on highlight
            "--color=prompt:#cdd6f4",     // Text - prompt color
            "--color=pointer:#f5e0dc",    // Rosewater - pointer
            "--color=border:#45475a",     // Surface1 - border color
            "--color=gutter:#1e1e2e",     // Base - gutter matches background
        ]);

    if let Some(index) = initial_index {
        builder = builder.initial_index(index);
    }

    match builder.select_padded(items)? {
        crate::menu_utils::FzfResult::Selected(item) => Ok(Some(item)),
        _ => Ok(None),
    }
}

pub fn select_one_with_style<T>(items: Vec<T>) -> Result<Option<T>>
where
    T: crate::menu_utils::FzfSelectable + Clone,
{
    select_one_with_style_at(items, None)
}
