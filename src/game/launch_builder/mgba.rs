//! mGBA-Qt (Game Boy Advance emulator) launch command builder
//!
//! Builds commands for running GBA/GB/GBC games via mGBA-Qt

use std::path::PathBuf;

use anyhow::Result;

use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

use super::validation::{format_valid_extensions, validate_mgba_file, MGBA_EXTENSIONS};

/// mGBA-Qt command name
const MGBA_COMMAND: &str = "mgba-qt";

pub struct MgbaBuilder;

impl MgbaBuilder {
    /// Build an mGBA-Qt launch command interactively
    pub fn build_command() -> Result<Option<String>> {
        // Step 1: Check if mGBA-Qt is installed
        if !Self::check_mgba_installed() {
            FzfWrapper::message(&format!(
                "{} mGBA-Qt not found!\n\n\
                 Install it with your package manager:\n\
                 • Arch: pacman -S mgba-qt\n\
                 • Ubuntu/Debian: apt install mgba-qt\n\
                 • Fedora: dnf install mgba-qt\n\n\
                 Or visit: https://mgba.io/downloads.html",
                char::from(NerdFont::CrossCircle)
            ))?;
            return Ok(None);
        }

        // Step 2: Select game file
        let game_file = match Self::select_game_file()? {
            Some(f) => f,
            None => return Ok(None),
        };

        // Step 3: Ask for fullscreen
        let fullscreen = Self::ask_fullscreen()?;

        // Build the command
        let command = Self::format_command(&game_file, fullscreen);

        // Show preview and confirm
        if Self::confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn check_mgba_installed() -> bool {
        which::which(MGBA_COMMAND).is_ok()
    }

    fn select_game_file() -> Result<Option<PathBuf>> {
        let selection = PathInputBuilder::new()
            .header(format!(
                "{} Select Game Boy Advance Game File",
                char::from(NerdFont::Gamepad)
            ))
            .scope(FilePickerScope::Files)
            .picker_hint(format!(
                "{} Select a GBA/GB/GBC game file ({})",
                char::from(NerdFont::Info),
                format_valid_extensions(MGBA_EXTENSIONS)
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
                if let Err(e) = validate_mgba_file(&path) {
                    FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                    return Ok(None);
                }
                Ok(Some(path))
            }
            PathInputSelection::Picker(path) => {
                if let Err(e) = validate_mgba_file(&path) {
                    FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                    return Ok(None);
                }
                Ok(Some(path))
            }
            PathInputSelection::WinePrefix(_) => Ok(None),
            PathInputSelection::Cancelled => Ok(None),
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

    fn format_command(game_file: &PathBuf, fullscreen: bool) -> String {
        let game_str = game_file.to_string_lossy();

        let mut parts = vec![MGBA_COMMAND.to_string()];

        if fullscreen {
            parts.push("-f".to_string());
        }

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
