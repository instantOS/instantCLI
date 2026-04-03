//! Eden (Switch emulator) launch command builder
//!
//! Builds commands for running Nintendo Switch games via Eden AppImage

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::game::launch_command::{
    EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, EmulatorPlatform, LaunchCommand,
    LaunchCommandKind,
};
use crate::game::platforms::appimage_finder::find_appimages_by_paths;
use crate::game::platforms::discovery::eden::collect_configured_rom_files;
use crate::ui::nerd_font::NerdFont;

use super::prompts::{
    AppImageSelectionPrompt, FileSelectionPrompt, ask_fullscreen, confirm_value,
    select_appimage_manually, select_detected_appimage, select_file_with_validation,
};
use super::validation::{EDEN_EXTENSIONS, format_valid_extensions, validate_game_file};

/// Default Eden AppImage location
/// (Matched case-insensitively - will find eden.AppImage, EDEN.APPIMAGE, etc.)
const DEFAULT_EDEN_PATH: &str = "~/AppImages/eden.appimage";

/// Alternative Eden AppImage locations to check
/// Filenames are matched case-insensitively, so eden.appimage will find
/// Eden.AppImage, EDEN.APPIMAGE, eden.appimage, etc.
const EDEN_SEARCH_PATHS: &[&str] = &[
    "~/AppImages/eden.appimage",
    "~/.local/bin/eden.appimage",
    "~/.local/share/applications/eden.appimage",
];

pub struct EdenBuilder;

impl EdenBuilder {
    /// Build an Eden launch command interactively
    pub fn build_command() -> Result<Option<LaunchCommand>> {
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
        let command = Self::build_launch_command(&eden_path, &game_file, fullscreen);

        // Show preview and confirm
        confirm_value(command)
    }

    pub(crate) fn find_or_select_eden() -> Result<Option<PathBuf>> {
        if let Some(path) = select_detected_appimage(
            &find_appimages_by_paths(EDEN_SEARCH_PATHS),
            NerdFont::Gamepad,
            "Eden",
        )? {
            return Ok(Some(path));
        }

        select_appimage_manually(AppImageSelectionPrompt::new(
            format!("{} Select Eden AppImage", char::from(NerdFont::Gamepad)),
            format!(
                "{} Select the Eden AppImage file (e.g., {})",
                char::from(NerdFont::Info),
                DEFAULT_EDEN_PATH
            ),
            "Eden AppImage not found at: {}".to_string(),
        ))
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
            )
            .suggested_paths(collect_configured_rom_files()),
            |path| validate_game_file(path, "Eden", EDEN_EXTENSIONS),
        )
    }

    fn build_launch_command(eden_path: &Path, game_file: &Path, fullscreen: bool) -> LaunchCommand {
        LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Eden,
                launcher: EmulatorLauncher::AppImage {
                    path: eden_path.to_path_buf(),
                },
                game: game_file.to_path_buf(),
                options: EmulatorOptions {
                    fullscreen,
                    batch_mode: false,
                },
            }),
        }
    }

    /// Format a simple Eden command without fullscreen flag.
    /// Used by the discovery prefill to avoid code duplication.
    pub(crate) fn format_command_simple(eden_path: &Path, game_file: &Path) -> LaunchCommand {
        Self::build_launch_command(eden_path, game_file, false)
    }
}
