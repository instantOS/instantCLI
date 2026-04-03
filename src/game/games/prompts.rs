use crate::common::TildePath;
use crate::game::games::validation::validate_non_empty;
use crate::game::utils::path::prompt_for_save_path;
use crate::menu_utils::{FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Result, anyhow};

pub(crate) fn get_game_description() -> Result<String> {
    Ok(FzfWrapper::input("Enter game description (optional)")
        .map_err(|e| anyhow!("Failed to get description input: {}", e))?
        .trim()
        .to_string())
}

pub(crate) fn get_launch_command() -> Result<String> {
    Ok(FzfWrapper::input("Enter launch command (optional)")
        .map_err(|e| anyhow!("Failed to get launch command input: {}", e))?
        .trim()
        .to_string())
}

pub(crate) fn get_save_path(game_name: &str) -> Result<TildePath> {
    prompt_for_save_path(game_name, || {
        let selection = PathInputBuilder::new()
            .header(format!(
                "{} Choose the save path for '{game_name}'",
                char::from(NerdFont::Folder)
            ))
            .manual_prompt(format!(
                "{} Enter the save path (e.g., ~/.local/share/{}/saves)",
                char::from(NerdFont::Edit),
                game_name.to_lowercase().replace(' ', "-")
            ))
            .scope(FilePickerScope::FilesAndDirectories)
            .picker_hint(format!(
                "{} Select the file or directory that stores the save data",
                char::from(NerdFont::Info)
            ))
            .manual_option_label(format!("{} Type an exact path", char::from(NerdFont::Edit)))
            .picker_option_label(format!(
                "{} Browse and choose a path",
                char::from(NerdFont::FolderOpen)
            ))
            .choose()?;

        match selection {
            PathInputSelection::Manual(input) => {
                if !validate_non_empty(&input, "Save path")? {
                    FzfWrapper::message("Save path cannot be empty.")?;
                    Ok(None)
                } else {
                    TildePath::from_str(&input)
                        .map(Some)
                        .map_err(|e| anyhow!("Invalid save path: {}", e))
                }
            }
            PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
                Ok(Some(TildePath::new(path)))
            }
            PathInputSelection::Cancelled => Ok(None),
        }
    })?
    .ok_or_else(|| anyhow!("Save path selection cancelled"))
}
