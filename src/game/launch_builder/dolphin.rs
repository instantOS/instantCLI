//! Dolphin (GameCube/Wii emulator) launch command builder via Flatpak
//!
//! Builds commands for running GameCube/Wii games via the Dolphin Flatpak

use anyhow::Result;
use std::path::PathBuf;

use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use super::flatpak::is_flatpak_app_installed;
use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{DOLPHIN_EXTENSIONS, format_valid_extensions, validate_dolphin_file};

/// Dolphin Flatpak application ID
const DOLPHIN_FLATPAK_ID: &str = "org.DolphinEmu.dolphin-emu";

pub struct DolphinBuilder;

impl DolphinBuilder {
    /// Build a Dolphin Flatpak launch command interactively
    pub fn build_command() -> Result<Option<String>> {
        // Step 1: Check if Dolphin Flatpak is installed
        if !Self::check_dolphin_installed()? {
            FzfWrapper::message(&format!(
                "{} Dolphin Flatpak not found!\n\n\
                 Install it with:\n\
                 flatpak install flathub {}\n\n\
                 Or visit: https://flathub.org/apps/org.DolphinEmu.dolphin-emu",
                char::from(NerdFont::CrossCircle),
                DOLPHIN_FLATPAK_ID
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

        // Step 4: Ask for fullscreen (only if not batch mode, as batch implies game-focused)
        let fullscreen = if !batch_mode {
            ask_fullscreen()?
        } else {
            false
        };

        // Build the command
        let command = Self::format_command(&game_file, batch_mode, fullscreen);

        // Show preview and confirm
        if confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn check_dolphin_installed() -> Result<bool> {
        is_flatpak_app_installed(DOLPHIN_FLATPAK_ID)
    }

    fn select_game_file() -> Result<Option<PathBuf>> {
        select_file_with_validation(
            FileSelectionPrompt::game_file(
                format!(
                    "{} Select GameCube/Wii Game File",
                    char::from(NerdFont::Disc)
                ),
                format!(
                    "{} Select a GameCube/Wii game file ({})",
                    char::from(NerdFont::Info),
                    format_valid_extensions(DOLPHIN_EXTENSIONS)
                ),
            ),
            validate_dolphin_file,
        )
    }

    fn ask_batch_mode() -> Result<bool> {
        match FzfWrapper::builder()
            .confirm(format!(
                "{} Use batch mode?\n\n\
                 Batch mode will exit Dolphin when the game closes.\n\
                 This is recommended for launching games directly.",
                char::from(NerdFont::Terminal)
            ))
            .yes_text("Yes, use batch mode")
            .no_text("No, keep Dolphin open")
            .confirm_dialog()?
        {
            ConfirmResult::Yes => Ok(true),
            _ => Ok(false),
        }
    }

    fn format_command(game_file: &PathBuf, batch_mode: bool, fullscreen: bool) -> String {
        let game_str = game_file.to_string_lossy();

        let mut parts = vec!["flatpak".to_string(), "run".to_string()];

        parts.push(DOLPHIN_FLATPAK_ID.to_string());

        if batch_mode {
            parts.push("-b".to_string());
        }

        if fullscreen {
            parts.push("-f".to_string());
        }

        parts.push("-e".to_string());
        parts.push(format!("\"{}\"", game_str));

        parts.join(" ")
    }
}
