//! PCSX2 (PlayStation 2 emulator) launch command builder
//!
//! Builds commands for running PS2 games via:
//! - EmuDeck AppImage (preferred, auto-detected)
//! - PCSX2 Flatpak (fallback)

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::game::launch_command::{
    EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, EmulatorPlatform, LaunchCommand,
    LaunchCommandKind,
};
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use super::appimage_finder::find_appimages_by_paths;
use super::flatpak::is_flatpak_app_installed;
use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_value, select_file_with_validation,
};
use super::validation::{PCSX2_EXTENSIONS, format_valid_extensions, validate_game_file};

/// PCSX2 Flatpak application ID
const PCSX2_FLATPAK_ID: &str = "net.pcsx2.PCSX2";

/// Common EmuDeck AppImage locations (case-insensitive matching)
/// All paths normalized to lowercase for comparison
const EMUDECK_APP_IMAGE_PATHS: &[&str] = &["~/emulation/tools/launchers/pcsx2-qt.appimage"];

/// Installation type for PCSX2
#[derive(Debug, Clone)]
enum Pcsx2InstallType {
    AppImage(PathBuf),
    Flatpak,
}

pub struct Pcsx2Builder;

impl Pcsx2Builder {
    /// Build a PCSX2 launch command interactively
    /// Prefers EmuDeck AppImage if found, falls back to Flatpak
    pub fn build_command() -> Result<Option<LaunchCommand>> {
        // Step 1: Detect installation type (AppImage preferred over Flatpak)
        let install_type = Self::detect_install_type()?;

        match install_type {
            None => {
                FzfWrapper::message(&format!(
                    "{} PCSX2 not found!\n\n\
                     Install one of the following:\n\n\
                     1. EmuDeck (includes PCSX2 AppImage)\n\
                        Visit: https://www.emudeck.com/\n\n\
                     2. PCSX2 Flatpak\n\
                        flatpak install flathub {}\n\
                        Or visit: https://flathub.org/apps/net.pcsx2.PCSX2",
                    char::from(NerdFont::CrossCircle),
                    PCSX2_FLATPAK_ID
                ))?;
                Ok(None)
            }
            Some(install_type) => {
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
                let command =
                    Self::build_launch_command(&install_type, &game_file, batch_mode, fullscreen);

                // Show preview and confirm
                confirm_value(command)
            }
        }
    }

    /// Detect the best available PCSX2 installation
    /// Returns AppImage path if found, otherwise checks for Flatpak
    fn detect_install_type() -> Result<Option<Pcsx2InstallType>> {
        // First, try to find the EmuDeck AppImage
        if let Some(appimage_path) = find_appimages_by_paths(EMUDECK_APP_IMAGE_PATHS)
            .into_iter()
            .next()
        {
            return Ok(Some(Pcsx2InstallType::AppImage(appimage_path)));
        }

        // Fall back to checking for Flatpak
        if is_flatpak_app_installed(PCSX2_FLATPAK_ID)? {
            return Ok(Some(Pcsx2InstallType::Flatpak));
        }

        Ok(None)
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
            |path| validate_game_file(path, "PCSX2", PCSX2_EXTENSIONS),
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

    fn build_launch_command(
        install_type: &Pcsx2InstallType,
        game_file: &Path,
        batch_mode: bool,
        fullscreen: bool,
    ) -> LaunchCommand {
        let _ = PCSX2_FLATPAK_ID;
        LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Pcsx2,
                launcher: match install_type {
                    Pcsx2InstallType::AppImage(path) => {
                        EmulatorLauncher::AppImage { path: path.clone() }
                    }
                    Pcsx2InstallType::Flatpak => EmulatorLauncher::Flatpak {
                        app_id: PCSX2_FLATPAK_ID,
                    },
                },
                game: game_file.to_path_buf(),
                options: EmulatorOptions {
                    batch_mode,
                    fullscreen,
                },
            }),
        }
    }
}
