//! Azahar (3DS emulator) launch command builder via Flatpak
//!
//! Builds commands for running 3DS games via the Azahar Flatpak

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;

use super::flatpak::is_flatpak_app_installed;
use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{AZAHAR_EXTENSIONS, format_valid_extensions, validate_azahar_file};

/// Azahar Flatpak application ID
const AZAHAR_FLATPAK_ID: &str = "org.azahar_emu.Azahar";

pub struct AzaharBuilder;

impl AzaharBuilder {
    /// Build an Azahar Flatpak launch command interactively
    pub fn build_command() -> Result<Option<String>> {
        // Step 1: Check if Azahar Flatpak is installed
        if !Self::check_azahar_installed()? {
            FzfWrapper::message(&format!(
                "{} Azahar Flatpak not found!


                 Install it with:

                 flatpak install flathub {}


                 Or visit: https://flathub.org/apps/org.azahar_emu.Azahar",
                char::from(NerdFont::CrossCircle),
                AZAHAR_FLATPAK_ID
            ))?;
            return Ok(None);
        }

        // Step 2: Select game file
        let game_file = match Self::select_game_file()? {
            Some(f) => f,
            None => return Ok(None),
        };

        // Step 3: Ask for fullscreen
        let fullscreen = ask_fullscreen()?;

        // Build the command
        let command = Self::format_command(&game_file, fullscreen);

        // Show preview and confirm
        if confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn check_azahar_installed() -> Result<bool> {
        is_flatpak_app_installed(AZAHAR_FLATPAK_ID)
    }

    fn select_game_file() -> Result<Option<PathBuf>> {
        select_file_with_validation(
            FileSelectionPrompt::game_file(
                format!(
                    "{} Select Nintendo 3DS Game File",
                    char::from(NerdFont::Gamepad)
                ),
                format!(
                    "{} Select a 3DS game file ({})",
                    char::from(NerdFont::Info),
                    format_valid_extensions(AZAHAR_EXTENSIONS)
                ),
            ),
            validate_azahar_file,
        )
    }

    fn format_command(game_file: &Path, fullscreen: bool) -> String {
        let game_str = game_file.to_string_lossy();

        let mut parts = vec!["flatpak".to_string(), "run".to_string()];

        parts.push(AZAHAR_FLATPAK_ID.to_string());

        if fullscreen {
            parts.push("-f".to_string());
        }

        // Use -- to signal end of options (in case filename starts with -)
        parts.push("--".to_string());
        parts.push(format!("\"{}\"", game_str));

        parts.join(" ")
    }
}
