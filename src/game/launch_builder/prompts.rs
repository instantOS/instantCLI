use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

pub(super) struct FileSelectionPrompt {
    pub header: String,
    pub picker_hint: String,
    pub manual_option_label: String,
    pub picker_option_label: String,
}

impl FileSelectionPrompt {
    pub(super) fn game_file(header: String, picker_hint: String) -> Self {
        Self {
            header,
            picker_hint,
            manual_option_label: format!("{} Type game file path", char::from(NerdFont::Edit)),
            picker_option_label: format!(
                "{} Browse for game file",
                char::from(NerdFont::FolderOpen)
            ),
        }
    }

    pub(super) fn new(
        header: String,
        picker_hint: String,
        manual_option_label: String,
        picker_option_label: String,
    ) -> Self {
        Self {
            header,
            picker_hint,
            manual_option_label,
            picker_option_label,
        }
    }
}

pub(super) fn select_file_with_validation<F>(
    prompt: FileSelectionPrompt,
    validate: F,
) -> Result<Option<PathBuf>>
where
    F: Fn(&Path) -> Result<(), String>,
{
    let selection = PathInputBuilder::new()
        .header(prompt.header)
        .scope(FilePickerScope::Files)
        .picker_hint(prompt.picker_hint)
        .manual_option_label(prompt.manual_option_label)
        .picker_option_label(prompt.picker_option_label)
        .choose()?;

    match selection {
        PathInputSelection::Manual(input) => {
            let path = PathBuf::from(shellexpand::tilde(&input).into_owned());
            if let Err(e) = validate(&path) {
                FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        PathInputSelection::Picker(path) => {
            if let Err(e) = validate(&path) {
                FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        PathInputSelection::WinePrefix(_) => Ok(None),
        PathInputSelection::Cancelled => Ok(None),
    }
}

pub(super) fn ask_fullscreen() -> Result<bool> {
    match FzfWrapper::confirm(&format!(
        "{} Run in fullscreen mode?",
        char::from(NerdFont::Fullscreen)
    ))? {
        ConfirmResult::Yes => Ok(true),
        _ => Ok(false),
    }
}

pub(super) fn confirm_command(command: &str) -> Result<bool> {
    let message = format!(
        "{} Generated Launch Command:\n\n{}\n\nUse this command?",
        char::from(NerdFont::Rocket),
        command
    );

    match FzfWrapper::confirm(&message)? {
        ConfirmResult::Yes => Ok(true),
        _ => Ok(false),
    }
}
