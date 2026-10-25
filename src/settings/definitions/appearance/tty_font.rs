//! System console fonts use vconsole.conf as their source of truth.
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};

use crate::common::console_font::{
    FONT_DIRECTORY, INSTALLED_NAME, configured_font, vconsole_config,
};
use crate::common::console_font_picker::{FontSession, pick_font};
use crate::menu_utils::DialogOutcome;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingState, SettingType};
use crate::ui::nerd_font::NerdFont;

pub struct TtyFont;

fn read_config() -> Result<String> {
    match std::fs::read_to_string("/etc/vconsole.conf") {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).context("Reading /etc/vconsole.conf"),
    }
}

impl Setting for TtyFont {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.tty_font")
            .title("TTY Font")
            .icon(NerdFont::Terminal)
            .summary("Choose the font for Linux virtual consoles and login TTYs.\n\nOn a Linux console, highlight fonts to preview them live; Escape restores the previous font. In a terminal emulator, font details are shown instead.\n\nSaves the font system-wide in /etc/vconsole.conf and applies it to TTYs. Your terminal emulator's font is configured separately.")
            .search_keywords(&["console", "font", "tty", "text size", "terminus"])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn get_display_state(&self, _ctx: &SettingsContext) -> SettingState {
        let label = match read_config().and_then(|text| configured_font(&text)) {
            Ok(Some(font)) if font == INSTALLED_NAME => "Previously selected font".to_string(),
            Ok(Some(font)) => font,
            Ok(None) => "System default".to_string(),
            Err(_) => "Unavailable".to_string(),
        };
        SettingState::Choice {
            current_label: label,
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = configured_font(&read_config()?)?;
        let console = crate::ui::catppuccin::linux_console_device();
        // Authenticate outside fzf so previews cannot block on a password
        // prompt. No console is touched while browsing from an emulator.
        if console.is_some() && !ctx.is_privileged() {
            ctx.run_command_as_root("true", std::iter::empty::<&str>())?;
        }
        let mut session = console
            .map(|console| FontSession::start(console.to_path_buf(), !ctx.is_privileged()))
            .transpose()
            .context("Preparing live TTY font preview")?;
        let DialogOutcome::Submitted(font) = pick_font(current.as_deref(), session.as_ref())?
        else {
            return Ok(());
        };

        // Stage the validated snapshot, then use the settings privilege wrapper
        // for system writes. Re-read the config after the picker to retain edits
        // made by localectl or another settings session while it was open.
        let staging = tempfile::tempdir().context("Staging TTY font configuration")?;
        crate::common::console_font::install_snapshot(staging.path(), &font)?;
        if font.data().is_some() {
            let source = staging
                .path()
                .join(format!("usr/share/kbd/consolefonts/{INSTALLED_NAME}.psf"));
            let destination = Path::new(FONT_DIRECTORY).join(format!("{INSTALLED_NAME}.psf"));
            ctx.run_command_as_root(
                "install",
                [Path::new("-D"), Path::new("-m644"), &source, &destination],
            )
            .context("Saving TTY font")?;
        }
        let config = staging.path().join("vconsole.conf");
        std::fs::write(&config, vconsole_config(&read_config()?, &font))?;
        ctx.run_command_as_root(
            "install",
            [Path::new("-m644"), &config, Path::new("/etc/vconsole.conf")],
        )
        .context("Saving TTY font configuration")?;

        // The selection is now persistent, even if refreshing other TTYs fails.
        if let Some(session) = session.as_mut() {
            session.confirm();
        }
        ctx.run_command_as_root("systemctl", ["restart", "systemd-vconsole-setup.service"])
            .context(
                "TTY font was saved, but could not be applied; it will be used at next boot",
            )?;
        ctx.emit_success(
            "settings.appearance.tty_font.saved",
            &format!("TTY font set to {}", font.label()),
        );
        Ok(())
    }
}
