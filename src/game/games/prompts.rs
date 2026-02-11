use crate::common::TildePath;
use crate::game::games::validation::validate_non_empty;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result, anyhow};

pub(crate) fn get_game_name(config: &crate::game::config::InstantGameConfig) -> Result<String> {
    let game_name = FzfWrapper::input("Enter game name")
        .map_err(|e| anyhow!("Failed to get game name input: {}", e))?
        .trim()
        .to_string();

    if !validate_non_empty(&game_name, "Game name")? {
        return Err(anyhow!("Game name cannot be empty"));
    }

    if config.games.iter().any(|g| g.name.0 == game_name) {
        eprintln!("Game '{game_name}' already exists!");
        return Err(anyhow!("Game already exists"));
    }

    Ok(game_name)
}

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

    let save_path = match selection {
        PathInputSelection::Manual(input) => {
            if !validate_non_empty(&input, "Save path")? {
                return Err(anyhow!("Save path cannot be empty"));
            }
            TildePath::from_str(&input).map_err(|e| anyhow!("Invalid save path: {}", e))?
        }
        PathInputSelection::Picker(path) => TildePath::new(path),
        PathInputSelection::WinePrefix(path) => TildePath::new(path),
        PathInputSelection::Cancelled => {
            println!("Game addition cancelled: save path not provided.");
            return Err(anyhow!("Save path selection cancelled"));
        }
    };

    if let Err(err) = ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory) {
        println!("{} {}", char::from(NerdFont::CrossCircle), err);
        return get_save_path(game_name);
    }

    let save_path_display = save_path
        .to_tilde_string()
        .unwrap_or_else(|_| save_path.as_path().to_string_lossy().to_string());

    match FzfWrapper::builder()
        .confirm(format!(
            "{} Are you sure you want to use '{save_path_display}' as the save path for '{game_name}'?\n\n\
            This path will be used to store and sync save files for this game.",
            char::from(NerdFont::Question)
        ))
        .yes_text("Use This Path")
        .no_text("Choose Different Path")
        .confirm_dialog()
        .map_err(|e| anyhow!("Failed to get path confirmation: {}", e))?
    {
        ConfirmResult::Yes => {}
        ConfirmResult::No | ConfirmResult::Cancelled => {
            println!("{} Choosing different save path...", char::from(NerdFont::Info));
            return get_save_path(game_name);
        }
    }

    if !save_path.as_path().exists() {
        match FzfWrapper::confirm(&format!(
            "{} Save path '{save_path_display}' does not exist. Create it?",
            char::from(NerdFont::Warning)
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
            ConfirmResult::No | ConfirmResult::Cancelled => {
                println!(
                    "{} Game addition cancelled: save path does not exist.",
                    char::from(NerdFont::Warning)
                );
                return Err(anyhow!("Save path does not exist"));
            }
        }
    }

    Ok(save_path)
}
