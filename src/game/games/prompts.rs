use crate::common::TildePath;
use crate::game::games::validation::validate_non_empty;
use crate::game::utils::path::prompt_for_save_path;
use crate::menu_utils::{
    ConfirmResult, DialogOutcome, FilePickerScope, FzfSelectable, FzfWrapper, HeaderBuilder,
    PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Result, anyhow};

pub(crate) fn unconfigured_saves_warning(game_name: &str) -> String {
    format!(
        "Backups not configured for '{game_name}'. Saves will not be backed up or restored until you configure their location.\n\nLaunch the game once to create its save files, then choose Set up saves in the game menu."
    )
}

#[derive(Clone, Copy)]
enum SaveSetupChoice {
    Configure,
    Later,
    Back,
}

impl FzfSelectable for SaveSetupChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Configure => "Configure save backups now",
            Self::Later => "Set up saves later (not backed up)",
            Self::Back => "Back",
        }
        .into()
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(match self {
            Self::Configure => "Choose the file or directory containing this game's saves.".into(),
            Self::Later => "Save the game and launch command without a save path.\n\nSaves will not be backed up. Run the game once, then use Set up saves in its menu to select the save location.".into(),
            Self::Back => "Cancel adding this game.".into(),
        })
    }
}

pub(crate) fn get_optional_save_path(game_name: &str) -> Result<DialogOutcome<Option<TildePath>>> {
    loop {
        let choice = FzfWrapper::builder()
            .header(format!("Save backups for '{game_name}'"))
            .items(vec![
                SaveSetupChoice::Configure,
                SaveSetupChoice::Later,
                SaveSetupChoice::Back,
            ])
            .select_one()?;
        match choice {
            DialogOutcome::Submitted(SaveSetupChoice::Configure) => {
                if let Some(path) = super::add::prompt_manual_save_path(game_name)? {
                    return Ok(DialogOutcome::Submitted(Some(path)));
                }
            }
            DialogOutcome::Submitted(SaveSetupChoice::Later) => {
                if FzfWrapper::builder()
                    .confirm(unconfigured_saves_warning(game_name))
                    .yes_text("Add without backups")
                    .no_text("Go back")
                    .confirm_dialog()?
                    == ConfirmResult::Yes
                {
                    return Ok(DialogOutcome::Submitted(None));
                }
            }
            DialogOutcome::Submitted(SaveSetupChoice::Back) | DialogOutcome::Cancelled => {
                return Ok(DialogOutcome::Cancelled);
            }
        }
    }
}

pub(crate) fn get_game_description() -> Result<String> {
    match FzfWrapper::input("Enter game description (optional)")
        .map_err(|e| anyhow!("Failed to get description input: {}", e))?
    {
        crate::menu_utils::DialogOutcome::Submitted(value) => Ok(value.trim().to_string()),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(String::new()),
    }
}

pub(crate) fn get_save_path(game_name: &str) -> Result<TildePath> {
    prompt_for_save_path(game_name, None, || {
        let selection = PathInputBuilder::new()
            .header(HeaderBuilder::new(
                NerdFont::Folder,
                format!("Choose the save path for '{game_name}'"),
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
                    Ok(Some(TildePath::from_str(&input)))
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
