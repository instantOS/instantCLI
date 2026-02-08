//! PCSX2 (PlayStation 2 emulator) launch command builder via Flatpak
//!
//! Builds commands for running PS2 games via the PCSX2 Flatpak

use anyhow::Result;
use std::path::PathBuf;

use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use super::flatpak::is_flatpak_app_installed;
use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{PCSX2_EXTENSIONS, format_valid_extensions, validate_pcsx2_file};

/// PCSX2 Flatpak application ID
const PCSX2_FLATPAK_ID: &str = "net.pcsx2.PCSX2";

pub struct Pcsx2Builder;

impl Pcsx2Builder {
    /// Build a PCSX2 Flatpak launch command interactively
    pub fn build_command() -> Result<Option<String>> {
        // Step 1: Check if PCSX2 Flatpak is installed
        if !Self::check_pcsx2_installed()? {
            FzfWrapper::message(&format!(
                "{} PCSX2 Flatpak not found!\n\n\
                 Install it with:\n\
                 flatpak install flathub {}\n\n\
                 Or visit: https://flathub.org/apps/net.pcsx2.PCSX2",
                char::from(NerdFont::CrossCircle),
                PCSX2_FLATPAK_ID
            ))?;
            return Ok(None);
        }

        // Step 2: Select game file
        let game_file = match Self::select_game_file()? {
            Some(f) => f,
            None => return Ok(None),
        };

        // Step 3: Ask for batch mode (exit when game closes)
        let batch_mode = Self::ask_batch_mode()?;

        // Step 4: Ask for fullscreen
        let fullscreen = ask_fullscreen()?;

        // Build the command
        let command = Self::format_command(&game_file, batch_mode, fullscreen);

        // Show preview and confirm
        if confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn check_pcsx2_installed() -> Result<bool> {
        is_flatpak_app_installed(PCSX2_FLATPAK_ID)
    }

    fn select_game_file() -> Result<Option<PathBuf>> {
        select_file_with_validation(
            FileSelectionPrompt::game_file(
                format!(
                    "{} Select PlayStation 2 Game File",
                    char::from(NerdFont::Disc)
                ),
                format!(
                    "{} Select a PS2 game file ({})",
                    char::from(NerdFont::Info),
                    format_valid_extensions(PCSX2_EXTENSIONS)
                ),
            ),
            validate_pcsx2_file,
        )
    }

    fn ask_batch_mode() -> Result<bool> {
        match FzfWrapper::builder()
            .confirm(format!(
                "{} Use batch mode?\n\n\
                 Batch mode will exit PCSX2 when the game closes.\n\
                 This is recommended for launching games directly.",
                char::from(NerdFont::Terminal)
            ))
            .yes_text("Yes, use batch mode")
            .no_text("No, keep PCSX2 open")
            .confirm_dialog()?
        {
            ConfirmResult::Yes => Ok(true),
            _ => Ok(false),
        }
    }

    fn format_command(game_file: &PathBuf, batch_mode: bool, fullscreen: bool) -> String {
        let game_str = game_file.to_string_lossy();

        let mut parts = vec!["flatpak".to_string(), "run".to_string()];

        parts.push(PCSX2_FLATPAK_ID.to_string());

        if batch_mode {
            parts.push("-batch".to_string());
        }

        if fullscreen {
            parts.push("-fullscreen".to_string());
        }

        // Use -- to signal end of options (in case filename starts with -)
        parts.push("--".to_string());
        parts.push(format!("\"{}\"", game_str));

        parts.join(" ")
    }
}
