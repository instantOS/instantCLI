//! Optional DuckStation AppImage acquisition when no installation is available.
use super::prompts::select_appimage_manually;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
pub(super) const DEFAULT_DUCKSTATION_PATH: &str = "~/AppImages/DuckStation-x64.AppImage";
const DUCKSTATION_DOWNLOAD_URL: &str =
    "https://github.com/stenzek/duckstation/releases/download/latest/DuckStation-x64.AppImage";
pub(super) fn select_or_download() -> Result<Option<PathBuf>> {
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
        ConfirmResult::Yes => download_duckstation(),
        ConfirmResult::No => select_appimage_manually("DuckStation"),
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
            fs::set_permissions(&dest_path, perms).context("Failed to make AppImage executable")?;

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
