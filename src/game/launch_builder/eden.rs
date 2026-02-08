//! Eden (Switch emulator) launch command builder
//!
//! Builds commands for running Nintendo Switch games via Eden AppImage

use std::path::PathBuf;

use anyhow::Result;

use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{EDEN_EXTENSIONS, format_valid_extensions, validate_eden_file};

/// Default Eden AppImage location
const DEFAULT_EDEN_PATH: &str = "~/AppImages/eden.AppImage";

/// Alternative Eden AppImage locations to check
const EDEN_SEARCH_PATHS: &[&str] = &[
    "~/AppImages/eden.AppImage",
    "~/AppImages/Eden.AppImage",
    "~/.local/bin/eden.AppImage",
    "~/.local/share/applications/eden.AppImage",
];

pub struct EdenBuilder;

impl EdenBuilder {
    /// Build an Eden launch command interactively
    pub fn build_command() -> Result<Option<String>> {
        // Step 1: Find or select Eden AppImage
        let eden_path = match Self::find_or_select_eden()? {
            Some(p) => p,
            None => return Ok(None),
        };

        // Step 2: Select game file
        let game_file = match Self::select_game_file()? {
            Some(f) => f,
            None => return Ok(None),
        };

        // Step 3: Ask for fullscreen
        let fullscreen = ask_fullscreen()?;

        // Build the command
        let command = Self::format_command(&eden_path, &game_file, fullscreen);

        // Show preview and confirm
        if confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn find_or_select_eden() -> Result<Option<PathBuf>> {
        // Try to find Eden in common locations
        for search_path in EDEN_SEARCH_PATHS {
            let expanded = shellexpand::tilde(search_path);
            let path = PathBuf::from(expanded.into_owned());
            if path.exists() && path.is_file() {
                // Found Eden, ask if user wants to use it
                match FzfWrapper::builder()
                    .confirm(format!(
                        "{} Found Eden at:\n{}\n\nUse this?",
                        char::from(NerdFont::Check),
                        path.display()
                    ))
                    .yes_text("Use This")
                    .no_text("Choose Different")
                    .confirm_dialog()?
                {
                    ConfirmResult::Yes => return Ok(Some(path)),
                    ConfirmResult::No => break,
                    ConfirmResult::Cancelled => return Ok(None),
                }
            }
        }

        // Not found or user wants different, let them select
        let selection = PathInputBuilder::new()
            .header(format!(
                "{} Select Eden AppImage",
                char::from(NerdFont::Gamepad)
            ))
            .scope(FilePickerScope::Files)
            .picker_hint(format!(
                "{} Select the Eden AppImage file (e.g., {})",
                char::from(NerdFont::Info),
                DEFAULT_EDEN_PATH
            ))
            .manual_option_label(format!("{} Type AppImage path", char::from(NerdFont::Edit)))
            .picker_option_label(format!(
                "{} Browse for AppImage",
                char::from(NerdFont::FolderOpen)
            ))
            .choose()?;

        match selection {
            PathInputSelection::Manual(input) => {
                let path = PathBuf::from(shellexpand::tilde(&input).into_owned());
                if !path.exists() {
                    FzfWrapper::message(&format!(
                        "{} Eden AppImage not found at: {}",
                        char::from(NerdFont::CrossCircle),
                        path.display()
                    ))?;
                    return Ok(None);
                }
                Ok(Some(path))
            }
            PathInputSelection::Picker(path) => {
                if !path.exists() {
                    FzfWrapper::message(&format!(
                        "{} File not found: {}",
                        char::from(NerdFont::CrossCircle),
                        path.display()
                    ))?;
                    return Ok(None);
                }
                Ok(Some(path))
            }
            PathInputSelection::WinePrefix(_) => Ok(None),
            PathInputSelection::Cancelled => Ok(None),
        }
    }

    fn select_game_file() -> Result<Option<PathBuf>> {
        select_file_with_validation(
            FileSelectionPrompt::game_file(
                format!("{} Select Switch Game File", char::from(NerdFont::Gamepad)),
                format!(
                    "{} Select a Switch game file ({})",
                    char::from(NerdFont::Info),
                    format_valid_extensions(EDEN_EXTENSIONS)
                ),
            ),
            validate_eden_file,
        )
    }

    fn format_command(eden_path: &PathBuf, game_file: &PathBuf, fullscreen: bool) -> String {
        let eden_str = eden_path.to_string_lossy();
        let game_str = game_file.to_string_lossy();

        let mut parts = vec![format!("\"{}\"", eden_str)];

        if fullscreen {
            parts.push("-f".to_string());
        }

        parts.push(format!("-g \"{}\"", game_str));

        parts.join(" ")
    }
}
