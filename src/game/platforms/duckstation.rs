//! DuckStation (PlayStation 1 emulator) launch command builder
//!
//! Builds commands for running PS1 games via DuckStation AppImage.
//! Downloads the AppImage automatically if not found.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::game::platforms::appimage_finder::find_appimage_by_paths;
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{DUCKSTATION_EXTENSIONS, format_valid_extensions, validate_game_file};

/// Default DuckStation AppImage location
const DEFAULT_DUCKSTATION_PATH: &str = "~/AppImages/DuckStation-x64.AppImage";

/// DuckStation download URL (x64 only)
const DUCKSTATION_DOWNLOAD_URL: &str =
    "https://github.com/stenzek/duckstation/releases/download/latest/DuckStation-x64.AppImage";

/// Alternative DuckStation AppImage locations to check
/// Filenames are matched case-insensitively, so duckstation.appimage will find
/// DuckStation-x64.AppImage, duckstation.appimage, DUCKSTATION.APPIMAGE, etc.
const DUCKSTATION_SEARCH_PATHS: &[&str] = &[
    "~/AppImages/DuckStation-x64.AppImage",
    "~/AppImages/duckstation.AppImage",
    "~/.local/bin/DuckStation-x64.AppImage",
    "~/.local/share/applications/DuckStation-x64.AppImage",
];

pub struct DuckStationBuilder;

impl DuckStationBuilder {
    /// Build a DuckStation launch command interactively
    pub fn build_command() -> Result<Option<String>> {
        // Step 1: Find or download DuckStation AppImage
        let duckstation_path = match Self::find_or_download_duckstation()? {
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

        // Step 4: Ask for batch mode (exit when game closes)
        let batch_mode = Self::ask_batch_mode()?;

        // Build the command
        let command = Self::format_command(&duckstation_path, &game_file, fullscreen, batch_mode);

        // Show preview and confirm
        if confirm_command(&command)? {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn find_or_download_duckstation() -> Result<Option<PathBuf>> {
        // Try to find DuckStation in common locations using case-insensitive matching
        if let Some(path) = find_appimage_by_paths(DUCKSTATION_SEARCH_PATHS) {
            // Found DuckStation, ask if user wants to use it
            match FzfWrapper::builder()
                .confirm(format!(
                    "{} Found DuckStation at:\n{}\n\nUse this?",
                    char::from(NerdFont::Check),
                    path.display()
                ))
                .yes_text("Use This")
                .no_text("Choose Different")
                .confirm_dialog()?
            {
                ConfirmResult::Yes => return Ok(Some(path)),
                ConfirmResult::No => {}
                ConfirmResult::Cancelled => return Ok(None),
            }
        }

        // Not found, offer to download or select manually
        Self::offer_download_or_select()
    }

    fn offer_download_or_select() -> Result<Option<PathBuf>> {
        let message = format!(
            "{} DuckStation AppImage not found!\n\n\
             Would you like to download it?\n\
             (x64/AMD64 only, ~80MB)\n\n\
             Download URL:\n{}",
            char::from(NerdFont::CloudDownload),
            DUCKSTATION_DOWNLOAD_URL
        );

        match FzfWrapper::builder()
            .confirm(message)
            .yes_text("Download DuckStation")
            .no_text("Select Manually")
            .confirm_dialog()?
        {
            ConfirmResult::Yes => Self::download_duckstation(),
            ConfirmResult::No => Self::select_duckstation_manually(),
            ConfirmResult::Cancelled => Ok(None),
        }
    }

    fn download_duckstation() -> Result<Option<PathBuf>> {
        let dest_path = PathBuf::from(shellexpand::tilde(DEFAULT_DUCKSTATION_PATH).into_owned());

        // Create AppImages directory if it doesn't exist
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).context("Failed to create AppImages directory")?;
        }

        FzfWrapper::message(&format!(
            "{} Downloading DuckStation...\n\n\
             This may take a few minutes.\n\
             Destination: {}",
            char::from(NerdFont::CloudDownload),
            dest_path.display()
        ))?;

        // Use curl or wget to download (synchronously, as this is simpler for AppImages)
        let download_result = if which::which("curl").is_ok() {
            Command::new("curl")
                .args(["-L", "-o"])
                .arg(&dest_path)
                .arg(DUCKSTATION_DOWNLOAD_URL)
                .status()
        } else if which::which("wget").is_ok() {
            Command::new("wget")
                .args(["-O"])
                .arg(&dest_path)
                .arg(DUCKSTATION_DOWNLOAD_URL)
                .status()
        } else {
            FzfWrapper::message(&format!(
                "{} Neither curl nor wget found!\n\n\
                 Please install curl or wget to download automatically,\n\
                 or download manually from:\n{}",
                char::from(NerdFont::CrossCircle),
                DUCKSTATION_DOWNLOAD_URL
            ))?;
            return Ok(None);
        };

        match download_result {
            Ok(status) if status.success() => {
                // Make it executable
                let mut perms = fs::metadata(&dest_path)
                    .context("Failed to get file metadata")?
                    .permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dest_path, perms)
                    .context("Failed to make AppImage executable")?;

                FzfWrapper::message(&format!(
                    "{} DuckStation downloaded successfully!\n\n\
                     Location: {}",
                    char::from(NerdFont::Check),
                    dest_path.display()
                ))?;

                Ok(Some(dest_path))
            }
            Ok(_) => {
                // Download failed
                FzfWrapper::message(&format!(
                    "{} Download failed!\n\n\
                     Please download manually from:\n{}",
                    char::from(NerdFont::CrossCircle),
                    DUCKSTATION_DOWNLOAD_URL
                ))?;

                // Clean up partial download
                let _ = fs::remove_file(&dest_path);
                Ok(None)
            }
            Err(e) => {
                FzfWrapper::message(&format!(
                    "{} Download error: {}\n\n\
                     Please download manually from:\n{}",
                    char::from(NerdFont::CrossCircle),
                    e,
                    DUCKSTATION_DOWNLOAD_URL
                ))?;
                Ok(None)
            }
        }
    }

    fn select_duckstation_manually() -> Result<Option<PathBuf>> {
        let selection = PathInputBuilder::new()
            .header(format!(
                "{} Select DuckStation AppImage",
                char::from(NerdFont::Disc)
            ))
            .scope(FilePickerScope::Files)
            .picker_hint(format!(
                "{} Select the DuckStation AppImage file (e.g., {})",
                char::from(NerdFont::Info),
                DEFAULT_DUCKSTATION_PATH
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
                        "{} DuckStation AppImage not found at: {}",
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
                format!(
                    "{} Select PlayStation 1 Game File",
                    char::from(NerdFont::Disc)
                ),
                format!(
                    "{} Select a PS1 game file ({})",
                    char::from(NerdFont::Info),
                    format_valid_extensions(DUCKSTATION_EXTENSIONS)
                ),
            ),
            |path| validate_game_file(path, "DuckStation", DUCKSTATION_EXTENSIONS),
        )
    }

    fn ask_batch_mode() -> Result<bool> {
        match FzfWrapper::builder()
            .confirm(format!(
                "{} Use batch mode?\n\n\
                 Batch mode will start the game directly without showing\n\
                 the DuckStation main window first.",
                char::from(NerdFont::Terminal)
            ))
            .yes_text("Yes, start game directly")
            .no_text("No, show main window")
            .confirm_dialog()?
        {
            ConfirmResult::Yes => Ok(true),
            _ => Ok(false),
        }
    }

    fn format_command(
        duckstation_path: &Path,
        game_file: &Path,
        fullscreen: bool,
        batch_mode: bool,
    ) -> String {
        let duckstation_str = duckstation_path.to_string_lossy();
        let game_str = game_file.to_string_lossy();

        let mut parts = vec![format!("\"{}\"", duckstation_str)];

        if batch_mode {
            parts.push("-batch".to_string());
        }

        if fullscreen {
            parts.push("-fullscreen".to_string());
        }

        parts.push("--".to_string());
        parts.push(format!("\"{}\"", game_str));

        parts.join(" ")
    }
}
