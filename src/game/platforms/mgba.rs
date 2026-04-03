//! mGBA-Qt (Game Boy Advance emulator) launch command builder
//!
//! Builds commands for running GBA/GB/GBC games via mGBA-Qt

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::game::launch_command::{
    EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, EmulatorPlatform, LaunchCommand,
    LaunchCommandKind,
};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;

use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_value, select_file_with_validation,
};

use super::validation::{MGBA_EXTENSIONS, format_valid_extensions, validate_game_file};

/// mGBA-Qt command name
const MGBA_COMMAND: &str = "mgba-qt";

pub struct MgbaBuilder;

impl MgbaBuilder {
    /// Build an mGBA-Qt launch command interactively
    pub fn build_command() -> Result<Option<LaunchCommand>> {
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
        let fullscreen = ask_fullscreen()?;

        // Build the command
        let command = Self::build_launch_command(&game_file, fullscreen);

        // Show preview and confirm
        confirm_value(command)
    }

    fn check_mgba_installed() -> bool {
        which::which(MGBA_COMMAND).is_ok()
    }

    fn select_game_file() -> Result<Option<PathBuf>> {
        select_file_with_validation(
            FileSelectionPrompt::game_file(
                format!(
                    "{} Select Game Boy Advance Game File",
                    char::from(NerdFont::Gamepad)
                ),
                format!(
                    "{} Select a GBA/GB/GBC game file ({})",
                    char::from(NerdFont::Info),
                    format_valid_extensions(MGBA_EXTENSIONS)
                ),
            ),
            |path| validate_game_file(path, "mGBA", MGBA_EXTENSIONS),
        )
    }

    fn build_launch_command(game_file: &Path, fullscreen: bool) -> LaunchCommand {
        let _ = MGBA_COMMAND;
        LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Mgba,
                launcher: EmulatorLauncher::Native {
                    command: MGBA_COMMAND,
                },
                game: game_file.to_path_buf(),
                options: EmulatorOptions {
                    fullscreen,
                    batch_mode: false,
                },
            }),
        }
    }
}
