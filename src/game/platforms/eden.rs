//! Eden (Switch emulator) launch command builder
//!
//! Builds commands for running Nintendo Switch games via Eden AppImage

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::game::launch_command::{
    EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, EmulatorPlatform, LaunchCommand,
    LaunchCommandKind,
};
use crate::game::platforms::appimage_finder::{find_appimage_by_paths, find_appimages_by_paths};
use crate::game::platforms::discovery::eden::collect_configured_rom_files;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{
    FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_value, select_file_with_validation,
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
        match Self::find_detected_eden_paths().as_slice() {
            [path] => return Ok(Some(path.clone())),
            paths if !paths.is_empty() => {
                if let Some(path) = Self::select_detected_eden_path(paths)? {
                    return Ok(Some(path));
                }
            }
            _ => {}
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

    pub(crate) fn find_eden_noninteractive() -> Option<PathBuf> {
        find_appimage_by_paths(EDEN_SEARCH_PATHS)
    }

    fn find_detected_eden_paths() -> Vec<PathBuf> {
        find_appimages_by_paths(EDEN_SEARCH_PATHS)
    }

    fn select_detected_eden_path(paths: &[PathBuf]) -> Result<Option<PathBuf>> {
        #[derive(Clone)]
        struct EdenPathItem {
            path: PathBuf,
        }

        impl FzfSelectable for EdenPathItem {
            fn fzf_display_text(&self) -> String {
                format!("{} {}", char::from(NerdFont::Check), self.path.display())
            }

            fn fzf_key(&self) -> String {
                self.path.to_string_lossy().into_owned()
            }

            fn fzf_preview(&self) -> FzfPreview {
                PreviewBuilder::new()
                    .header(NerdFont::Gamepad, "Detected Eden AppImage")
                    .text("Multiple Eden AppImages were found.")
                    .blank()
                    .field("Path", &self.path.display().to_string())
                    .build()
            }
        }

        let items: Vec<EdenPathItem> = paths
            .iter()
            .cloned()
            .map(|path| EdenPathItem { path })
            .collect();

        match FzfWrapper::builder()
            .header(format!(
                "{} Select Eden AppImage",
                char::from(NerdFont::Gamepad)
            ))
            .prompt("Eden")
            .select(items)?
        {
            FzfResult::Selected(item) => Ok(Some(item.path)),
            FzfResult::Cancelled => Ok(None),
            _ => Ok(None),
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
