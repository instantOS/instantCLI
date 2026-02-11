use crate::common::TildePath;
use crate::game::config::{GameInstallation, PathContentKind};
use crate::game::games::prompts;
use crate::game::games::validation::validate_non_empty;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result, anyhow};
use std::fs;

pub(super) fn resolve_relocation_game_name(game_name: Option<String>) -> Result<Option<String>> {
    Ok(match game_name {
        Some(name) => Some(name),
        None => {
            crate::game::games::selection::select_game_interactive(Some("Select a game to move:"))?
        }
    })
}

pub(super) fn ensure_game_exists(
    context: &super::manager::GameCreationContext,
    game_name: &str,
) -> Result<bool> {
    if !context.game_exists(game_name) {
        eprintln!("Game '{game_name}' not found in configuration.");
        Ok(false)
    } else {
        Ok(true)
    }
}

pub(super) fn resolve_relocation_save_path(
    game_name: &str,
    new_path: Option<String>,
) -> Result<TildePath> {
    match new_path {
        Some(path) => resolve_manual_save_path(path),
        None => prompts::get_save_path(game_name),
    }
}

fn resolve_manual_save_path(path: String) -> Result<TildePath> {
    let trimmed = path.trim();
    if !validate_non_empty(trimmed, "Save path")? {
        return Err(anyhow!("Save path cannot be empty"));
    }
    let tilde_path =
        TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid save path: {}", e))?;

    ensure_safe_path(tilde_path.as_path(), PathUsage::SaveDirectory)?;

    if !tilde_path.as_path().exists() {
        match FzfWrapper::confirm(&format!(
            "{} Save path '{}' does not exist. Create it?",
            char::from(NerdFont::Warning),
            trimmed
        ))
        .map_err(|e| anyhow!("Failed to get confirmation: {}", e))?
        {
            ConfirmResult::Yes => {
                std::fs::create_dir_all(tilde_path.as_path())
                    .context("Failed to create save directory")?;
                println!(
                    "{} Created save directory: {}",
                    char::from(NerdFont::Check),
                    trimmed
                );
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                return Err(anyhow!("Save path creation cancelled"));
            }
        }
    }

    Ok(tilde_path)
}

pub(super) fn determine_save_path_type(save_path: &TildePath) -> Result<PathContentKind> {
    if save_path.as_path().exists() {
        let metadata = fs::metadata(save_path.as_path()).with_context(|| {
            format!(
                "Failed to read metadata for save path: {}",
                save_path.as_path().display()
            )
        })?;
        Ok(PathContentKind::from(metadata))
    } else {
        Ok(PathContentKind::Directory)
    }
}

pub(super) fn upsert_installation(
    installations: &mut Vec<GameInstallation>,
    game_name: &str,
    save_path: TildePath,
    save_path_type: PathContentKind,
) {
    if let Some(inst) = installations
        .iter_mut()
        .find(|i| i.game_name.0 == game_name)
    {
        inst.save_path = save_path;
        inst.save_path_type = save_path_type;
        inst.nearest_checkpoint = None;
    } else {
        installations.push(GameInstallation::with_kind(
            game_name.to_string(),
            save_path,
            save_path_type,
        ));
    }
}
