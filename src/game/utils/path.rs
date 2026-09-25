use anyhow::{Context, Result, anyhow};
use std::path::Path;

use crate::common::TildePath;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::game::utils::save_files::{format_file_size, path_size_reaches};
use crate::menu_utils::{ConfirmResult, FzfWrapper, PathInputSelection};
use crate::ui::nerd_font::NerdFont;

/// Save paths larger than this are treated as suspicious: they likely hold the
/// whole game instead of just its saves, which makes backup and sync very slow.
const LARGE_SAVE_PATH_BYTES: u64 = 100 * 1024 * 1024;

/// Convert a PathInputSelection into a TildePath
/// Returns None if the selection was cancelled or empty
pub fn path_selection_to_tilde(selection: PathInputSelection) -> Result<Option<TildePath>> {
    match selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(TildePath::from_str(trimmed)))
            }
        }
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
            Ok(Some(TildePath::new(path)))
        }
        PathInputSelection::Cancelled => Ok(None),
    }
}

/// Build the warning message shown when a selected save path is very large.
///
/// Returns None when the path does not exist yet or is small enough; the
/// warning is best-effort and should never block path selection.
fn large_save_path_warning(path: &Path) -> Option<String> {
    if !path.exists() || !path_size_reaches(path, LARGE_SAVE_PATH_BYTES) {
        return None;
    }

    Some(format!(
        "{} '{}' contains at least {} of data.\n\nA save path is intended to store game saves, not the entire game. Keeping the whole game (or a library) here will make backups and syncing very slow.\n\nConsider picking a directory that contains only this game's save data.",
        char::from(NerdFont::Warning),
        TildePath::new(path.to_path_buf()).display_string(),
        format_file_size(LARGE_SAVE_PATH_BYTES),
    ))
}

pub fn prompt_for_save_path<F>(
    game_name: &str,
    current_path: Option<&TildePath>,
    mut select_path: F,
) -> Result<Option<TildePath>>
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

        // If the path is the same as the current one, just return it without confirmation
        if let Some(current) = current_path
            && current == &save_path
        {
            return Ok(Some(save_path));
        }

        // Warn before confirming when the path looks too large to be just saves
        if let Some(warning) = large_save_path_warning(save_path.as_path()) {
            FzfWrapper::message(&warning)?;
        }

        let save_path_display = save_path.display_string();

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    /// Create a sparse file that reports `len` bytes without occupying disk space
    fn write_sparse_file(path: &Path, len: u64) {
        File::create(path)
            .and_then(|file| file.set_len(len))
            .expect("failed to create sparse test file");
    }

    #[test]
    fn small_save_path_produces_no_warning() {
        let temp = TempDir::new().expect("failed to create temp dir");
        write_sparse_file(&temp.path().join("save1.dat"), 1024);

        assert!(large_save_path_warning(temp.path()).is_none());
    }

    #[test]
    fn large_save_path_produces_warning() {
        let temp = TempDir::new().expect("failed to create temp dir");
        write_sparse_file(&temp.path().join("huge.sav"), LARGE_SAVE_PATH_BYTES);

        let warning = large_save_path_warning(temp.path()).expect("expected a warning message");
        assert!(warning.contains("100.0 MB"), "size missing from: {warning}");
        assert!(
            warning.contains("not the entire game"),
            "guidance missing from: {warning}"
        );
    }

    #[test]
    fn missing_save_path_produces_no_warning() {
        let temp = TempDir::new().expect("failed to create temp dir");

        assert!(large_save_path_warning(&temp.path().join("missing")).is_none());
    }
}
