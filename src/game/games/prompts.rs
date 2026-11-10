use crate::common::TildePath;
use crate::game::games::validation::validate_non_empty;
use crate::game::utils::path::prompt_for_save_path;
use crate::menu_utils::{
    ConfirmResult, DialogOutcome, FilePickerScope, FzfSelectable, FzfWrapper, HeaderBuilder,
    PathInputBuilder, PathInputSelection,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
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
            Self::Configure => format!(
                "{} Configure save backups now",
                format_icon_colored(NerdFont::CloudSync, colors::GREEN)
            ),
            Self::Later => format!(
                "{} Set up saves later {}",
                format_icon_colored(NerdFont::Warning, colors::YELLOW),
                format_with_color("(not backed up)", colors::YELLOW)
            ),
            Self::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Configure => "configure",
            Self::Later => "later",
            Self::Back => "back",
        }
        .into()
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            Self::Configure => PreviewBuilder::new()
                .header(NerdFont::CloudSync, "Configure Save Backups")
                .text("Choose the file or directory containing this game's saves.")
                .blank()
                .bullet("Browse for save data or enter its exact path")
                .bullet("Create the save directory if it doesn't exist yet")
                .blank()
                .subtext("Only the selected save data will be tracked.")
                .build(),
            Self::Later => PreviewBuilder::new()
                .header(NerdFont::Warning, "Set Up Saves Later")
                .text("Keep the game and launch command without a save path.")
                .blank()
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Warning),
                    "Saves will not be backed up.",
                )
                .blank()
                .text("Run the game once to create its save files, then")
                .text("choose Set up saves in the game's menu.")
                .blank()
                .subtext("You'll also be offered save setup after launching.")
                .build(),
            Self::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Cancel adding this game.")
                .build(),
        }
    }
}

pub(crate) fn get_optional_save_path(game_name: &str) -> Result<DialogOutcome<Option<TildePath>>> {
    loop {
        let choice = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(
                    NerdFont::CloudSync,
                    format!("Save backups for '{game_name}'"),
                )
                .subtitle("Configure backups now or finish save setup after the first launch.")
                .build(),
            )
            .prompt("Save setup")
            .responsive_layout()
            .items(vec![
                SaveSetupChoice::Configure,
                SaveSetupChoice::Later,
                SaveSetupChoice::Back,
            ])
            .padded()
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
