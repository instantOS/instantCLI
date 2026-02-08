//! Dolphin (GameCube/Wii emulator) launch command builder via Flatpak
//!
//! Builds commands for running GameCube/Wii games via the Dolphin Flatpak

use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;

use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

use super::validation::{format_valid_extensions, validate_dolphin_file, DOLPHIN_EXTENSIONS};

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
            Self::ask_fullscreen()?
        } else {
            false
        };

        // Build the command
        let command = Self::format_command(&game_file, batch_mode, fullscreen);

        // Show preview and confirm
        if Self::confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn check_dolphin_installed() -> Result<bool> {
        let output = Command::new("flatpak")
            .args(["list", "--app", "--columns=application"])
            .output();

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(stdout.lines().any(|line| line.trim() == DOLPHIN_FLATPAK_ID))
            }
            Err(_) => {
                // flatpak command not found
                Ok(false)
            }
        }
    }

    fn select_game_file() -> Result<Option<PathBuf>> {
        let selection = PathInputBuilder::new()
            .header(format!(
                "{} Select GameCube/Wii Game File",
                char::from(NerdFont::Disc)
            ))
            .scope(FilePickerScope::Files)
            .picker_hint(format!(
                "{} Select a GameCube/Wii game file ({})",
                char::from(NerdFont::Info),
                format_valid_extensions(DOLPHIN_EXTENSIONS)
            ))
            .manual_option_label(format!(
                "{} Type game file path",
                char::from(NerdFont::Edit)
            ))
            .picker_option_label(format!(
                "{} Browse for game file",
                char::from(NerdFont::FolderOpen)
            ))
            .choose()?;

        match selection {
            PathInputSelection::Manual(input) => {
                let path = PathBuf::from(shellexpand::tilde(&input).into_owned());
                if let Err(e) = validate_dolphin_file(&path) {
                    FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                    return Ok(None);
                }
                Ok(Some(path))
            }
            PathInputSelection::Picker(path) => {
                if let Err(e) = validate_dolphin_file(&path) {
                    FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                    return Ok(None);
                }
                Ok(Some(path))
            }
            PathInputSelection::WinePrefix(_) => Ok(None),
            PathInputSelection::Cancelled => Ok(None),
        }
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

    fn ask_fullscreen() -> Result<bool> {
        match FzfWrapper::confirm(&format!(
            "{} Run in fullscreen mode?",
            char::from(NerdFont::Fullscreen)
        ))? {
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

    fn confirm_command(command: &str) -> Result<bool> {
        let message = format!(
            "{} Generated Launch Command:\n\n{}\n\nUse this command?",
            char::from(NerdFont::Rocket),
            command
        );

        match FzfWrapper::confirm(&message)? {
            ConfirmResult::Yes => Ok(true),
            _ => Ok(false),
        }
    }
}
