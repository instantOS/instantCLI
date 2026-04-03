use super::manager::GameCreationContext;
use crate::common::TildePath;
use crate::game::config::PathContentKind;
use crate::game::utils::path::prompt_for_save_path;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    FilePickerScope, FzfResult, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result, anyhow};
use std::fs;

#[derive(Debug, Default, Clone)]
pub struct AddGameOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub launch_command: Option<String>,
    pub save_path: Option<String>,
    pub create_save_path: bool,
    pub no_cache: bool,
}

pub(super) struct ResolvedGameDetails {
    pub(super) name: String,
    pub(super) description: Option<String>,
    pub(super) launch_command: Option<String>,
    pub(super) save_path: TildePath,
    pub(super) save_path_type: PathContentKind,
}

pub(super) fn resolve_add_game_details(
    options: AddGameOptions,
    context: &GameCreationContext,
) -> Result<Option<ResolvedGameDetails>> {
    let interactive_prompts = options.name.is_none();

    let AddGameOptions {
        name,
        description,
        launch_command,
        save_path,
        create_save_path,
        no_cache: _,
    } = options;

    let game_name = match name {
        Some(raw_name) => {
            let trimmed = raw_name.trim();
            if !super::validation::validate_non_empty(trimmed, "Game name")? {
                return Err(anyhow!("Game name cannot be empty"));
            }

            if context.game_exists(trimmed) {
                return Err(anyhow!("Game '{}' already exists", trimmed));
            }

            trimmed.to_string()
        }
        None => match prompt_manual_game_name(context)? {
            Some(name) => name,
            None => return Ok(None),
        },
    };

    let description = match description {
        Some(text) => some_if_not_empty(text),
        None if interactive_prompts => {
            match prompt_optional_text("Enter game description (optional)")? {
                Some(text) => some_if_not_empty(text),
                None => return Ok(None),
            }
        }
        None => None,
    };

    let launch_command = match launch_command {
        Some(command) => some_if_not_empty(command),
        None if interactive_prompts => {
            match prompt_optional_text("Enter launch command (optional)")? {
                Some(command) => some_if_not_empty(command),
                None => return Ok(None),
            }
        }
        None => None,
    };

    let save_path = match save_path {
        Some(path) => {
            let trimmed = path.trim();
            if !super::validation::validate_non_empty(trimmed, "Save path")? {
                return Err(anyhow!("Save path cannot be empty"));
            }

            let tilde_path =
                TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid save path: {}", e))?;

            ensure_safe_path(tilde_path.as_path(), PathUsage::SaveDirectory)?;

            if !tilde_path.as_path().exists() {
                if create_save_path {
                    fs::create_dir_all(tilde_path.as_path())
                        .context("Failed to create save directory")?;
                    println!(
                        "{} Created save directory: {}",
                        char::from(NerdFont::Check),
                        trimmed
                    );
                } else {
                    return Err(anyhow!(
                        "Save path '{}' does not exist. Use --create-save-path to create it automatically or run '{} game add' without --save-path for interactive setup.",
                        tilde_path.as_path().display(),
                        env!("CARGO_BIN_NAME")
                    ));
                }
            }

            tilde_path
        }
        None => match prompt_manual_save_path(&game_name)? {
            Some(path) => path,
            None => return Ok(None),
        },
    };

    ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory)?;

    let save_path_type = super::relocate::determine_save_path_type(&save_path)?;

    Ok(Some(ResolvedGameDetails {
        name: game_name,
        description,
        launch_command,
        save_path,
        save_path_type,
    }))
}

fn some_if_not_empty(value: impl Into<String>) -> Option<String> {
    let text = value.into();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn prompt_manual_game_name(context: &GameCreationContext) -> Result<Option<String>> {
    loop {
        let result = FzfWrapper::builder()
            .prompt("Enter game name")
            .input()
            .input_result()?;

        let game_name = match result {
            FzfResult::Selected(name) => name.trim().to_string(),
            FzfResult::Cancelled => return Ok(None),
            _ => return Ok(None),
        };

        if game_name.is_empty() {
            FzfWrapper::message("Game name cannot be empty.")?;
            continue;
        }

        if context.game_exists(&game_name) {
            FzfWrapper::message(&format!("Game '{}' already exists.", game_name))?;
            continue;
        }

        return Ok(Some(game_name));
    }
}

fn prompt_optional_text(prompt: &str) -> Result<Option<String>> {
    match FzfWrapper::builder()
        .prompt(prompt)
        .input()
        .input_result()?
    {
        FzfResult::Selected(value) => Ok(Some(value.trim().to_string())),
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

fn prompt_manual_save_path(game_name: &str) -> Result<Option<TildePath>> {
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
                if !super::validation::validate_non_empty(&input, "Save path")? {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
}
