use anyhow::{Context, Result, anyhow};
use std::path::Path;

use crate::common::TildePath;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{ConfirmResult, FzfWrapper, PathInputSelection};
use crate::ui::nerd_font::NerdFont;

/// Convert a TildePath to a display string, falling back to absolute path if tilde conversion fails
pub fn tilde_display_string(tilde: &TildePath) -> String {
    tilde
        .to_tilde_string()
        .unwrap_or_else(|_| tilde.as_path().to_string_lossy().to_string())
}

/// Convert a PathInputSelection into a TildePath
/// Returns None if the selection was cancelled or empty
pub fn path_selection_to_tilde(selection: PathInputSelection) -> Result<Option<TildePath>> {
    match selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(
                    TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid path: {e}"))?,
                ))
            }
        }
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
            Ok(Some(TildePath::new(path)))
        }
        PathInputSelection::Cancelled => Ok(None),
    }
}

pub fn prompt_for_save_path<F>(game_name: &str, mut select_path: F) -> Result<Option<TildePath>>
where
    F: FnMut() -> Result<Option<TildePath>>,
{
    loop {
        let Some(save_path) = select_path()? else {
            return Ok(None);
        };

        if let Err(err) = ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory) {
            FzfWrapper::message(&err.to_string())?;
            continue;
        }

        let save_path_display = tilde_display_string(&save_path);

        match FzfWrapper::builder()
            .confirm(format!(
                "{} Are you sure you want to use '{save_path_display}' as the save path for '{game_name}'?\n\nThis path will be used to store and sync save files for this game.",
                char::from(NerdFont::Question)
            ))
            .yes_text("Use This Path")
            .no_text("Choose Different Path")
            .confirm_dialog()
            .map_err(|e| anyhow!("Failed to get path confirmation: {}", e))?
        {
            ConfirmResult::Yes => {}
            ConfirmResult::No => continue,
            ConfirmResult::Cancelled => return Ok(None),
        }

        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "{} Save path '{}' does not exist. Create it?",
                char::from(NerdFont::Warning),
                save_path_display
            ))
            .map_err(|e| anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => {
                    std::fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    println!(
                        "{} Created save directory: {save_path_display}",
                        char::from(NerdFont::Check)
                    );
                }
                ConfirmResult::No => continue,
                ConfirmResult::Cancelled => return Ok(None),
            }
        }

        return Ok(Some(save_path));
    }
}

/// Validates that a path is a valid Wine prefix by checking for the presence of a drive_c directory
pub fn is_valid_wine_prefix(path: &Path) -> bool {
    let drive_c_path = path.join("drive_c");
    drive_c_path.exists() && drive_c_path.is_dir()
}

/// Checks if a path appears to be from a Wine prefix
/// Looks for common Wine directory patterns
pub fn is_wine_prefix_path(path: &str) -> bool {
    // Check for drive_c in the path (case-insensitive for robustness)
    let path_lower = path.to_lowercase();
    if !path_lower.contains("/drive_c/") {
        return false;
    }

    // Common Wine directory patterns
    path_lower.contains("/appdata/")
        || path_lower.contains("/users/")
        || path_lower.contains("/program files")
}
