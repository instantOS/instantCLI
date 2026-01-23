use anyhow::{anyhow, Result};

use crate::game::utils::path::{path_selection_to_tilde, tilde_display_string};
use crate::game::utils::safeguards::{ensure_safe_path, PathUsage};
use crate::menu_utils::{
    prompt_text_edit, FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, PathInputBuilder,
    TextEditOutcome, TextEditPrompt,
};
use crate::ui::nerd_font::NerdFont;

use super::state::EditState;

/// Edit the game name
pub fn edit_name(state: &mut EditState) -> Result<bool> {
    let current_name = &state.game().name.0;

    let result = FzfWrapper::builder()
        .prompt("Enter new game name")
        .header(format!("Current name: {}", current_name))
        .input()
        .query(current_name)
        .input_result()?;

    let new_name = match result {
        FzfResult::Selected(name) => name,
        FzfResult::Cancelled => {
            FzfWrapper::message("Edit cancelled. Name unchanged.")?;
            return Ok(false);
        }
        _ => return Ok(false),
    };

    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        FzfWrapper::message("Name cannot be empty. No changes made.")?;
        return Ok(false);
    }

    if trimmed == current_name {
        FzfWrapper::message("Name unchanged.")?;
        return Ok(false);
    }

    // Check for duplicates
    if state.game_config.games.iter().any(|g| g.name.0 == trimmed) {
        FzfWrapper::message(&format!("A game with name '{}' already exists.", trimmed))?;
        return Ok(false);
    }

    state.game_mut().name.0 = trimmed.to_string();
    FzfWrapper::message(&format!("Name updated to '{}'", trimmed))?;
    Ok(true)
}

/// Edit the game description
pub fn edit_description(state: &mut EditState) -> Result<bool> {
    let current_desc = state.game().description.clone();
    let current_desc_str = current_desc.as_deref();
    let current_display = match current_desc_str {
        Some(value) if !value.trim().is_empty() => value,
        _ => "<not set>",
    };

    let prompt = TextEditPrompt::new("Description", current_desc_str)
        .header(format!("Current description: {}", current_display))
        .ghost("Leave empty to remove");

    edit_optional_text_value(
        prompt,
        current_desc_str,
        "Description already empty.",
        "Description removed",
        "Description unchanged.",
        "Edit cancelled. Description unchanged.",
        "Description updated",
        |value| state.game_mut().description = value,
    )
}

/// Edit launch command (shows submenu for shared vs installation override)
pub fn edit_launch_command(state: &mut EditState) -> Result<bool> {
    let game_cmd = state.game().launch_command.as_deref();
    let inst_cmd = state
        .installation()
        .and_then(|i| i.launch_command.as_deref());

    // Build submenu
    #[derive(Debug, Clone)]
    enum LaunchCommandTarget {
        GameConfig,
        Installation,
        Back,
    }

    #[derive(Debug, Clone)]
    struct LaunchCommandOption {
        display: String,
        preview: String,
        target: LaunchCommandTarget,
    }

    impl FzfSelectable for LaunchCommandOption {
        fn fzf_display_text(&self) -> String {
            self.display.clone()
        }

        fn fzf_key(&self) -> String {
            self.display.clone()
        }

        fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
            crate::menu::protocol::FzfPreview::Text(self.preview.clone())
        }
    }

    let mut options = vec![LaunchCommandOption {
        display: format!(
            "{} Edit shared command (games.toml): {}",
            char::from(NerdFont::Edit),
            game_cmd.unwrap_or("<not set>")
        ),
        preview: format!(
            "Edit the launch command in games.toml\n\nCurrent value: {}\n\nThis command is shared across all devices.",
            game_cmd.unwrap_or("<not set>")
        ),
        target: LaunchCommandTarget::GameConfig,
    }];

    if state.installation_index.is_some() {
        options.push(LaunchCommandOption {
            display: format!(
                "{} Edit device-specific override (installations.toml): {}",
                char::from(NerdFont::Desktop),
                inst_cmd.unwrap_or("<not set>")
            ),
            preview: format!(
                "Edit the launch command override in installations.toml\n\nCurrent value: {}\n\nThis command is device-specific and overrides the shared command.",
                inst_cmd.unwrap_or("<not set>")
            ),
            target: LaunchCommandTarget::Installation,
        });
    }

    options.push(LaunchCommandOption {
        display: format!("{} Back", char::from(NerdFont::ArrowLeft)),
        preview: "Go back to main menu".to_string(),
        target: LaunchCommandTarget::Back,
    });

    let selection = FzfWrapper::builder()
        .header("Choose which launch command to edit")
        .select(options)?;

    match selection {
        FzfResult::Selected(option) => match option.target {
            LaunchCommandTarget::GameConfig => edit_game_launch_command(state),
            LaunchCommandTarget::Installation => edit_installation_launch_command(state),
            LaunchCommandTarget::Back => Ok(false),
        },
        _ => Ok(false),
    }
}

/// Edit the shared launch command in games.toml
fn edit_game_launch_command(state: &mut EditState) -> Result<bool> {
    let current_owned = state.game().launch_command.clone();
    let current = current_owned.as_deref();
    let header = format!("Current command: {}", current.unwrap_or("<not set>"));

    edit_optional_text_value(
        TextEditPrompt::new("Launch command", current)
            .header(header)
            .ghost("Leave empty to remove"),
        current,
        "Launch command already empty.",
        "Launch command removed from games.toml",
        "Launch command unchanged.",
        "Edit cancelled. Launch command unchanged.",
        "Launch command updated in games.toml",
        |value| state.game_mut().launch_command = value,
    )
}

/// Edit the installation-specific launch command override
fn edit_installation_launch_command(state: &mut EditState) -> Result<bool> {
    if state.installation().is_none() {
        return Err(anyhow!("No installation found for this game"));
    }

    let current_owned = state
        .installation()
        .and_then(|install| install.launch_command.clone());
    let current = current_owned.as_deref();
    let header = format!("Current override: {}", current.unwrap_or("<not set>"));

    edit_optional_text_value(
        TextEditPrompt::new("Launch command override", current)
            .header(header)
            .ghost("Leave empty to remove override"),
        current,
        "Launch command override already empty.",
        "Launch command override removed from installations.toml",
        "Launch command override unchanged.",
        "Edit cancelled. Launch command override unchanged.",
        "Launch command override updated in installations.toml",
        |value| {
            if let Some(installation) = state.installation_mut() {
                installation.launch_command = value;
            }
        },
    )
}

/// Edit the save path
pub fn edit_save_path(state: &mut EditState) -> Result<bool> {
    let installation = state
        .installation()
        .ok_or_else(|| anyhow!("No installation found for this game on this device"))?;

    let current_path = &installation.save_path;
    let current_path_str = tilde_display_string(current_path);

    let path_selection = PathInputBuilder::new()
        .header(format!(
            "{} Choose new save path\nCurrent: {}",
            char::from(NerdFont::Folder),
            current_path_str
        ))
        .manual_prompt(format!(
            "{} Enter the new save path:",
            char::from(NerdFont::Edit)
        ))
        .scope(FilePickerScope::FilesAndDirectories)
        .picker_hint(format!(
            "{} Select the file or directory to use for save data",
            char::from(NerdFont::Info)
        ))
        .manual_option_label(format!("{} Type an exact path", char::from(NerdFont::Edit)))
        .picker_option_label(format!(
            "{} Browse and choose a path",
            char::from(NerdFont::FolderOpen)
        ))
        .choose()?;

    match path_selection_to_tilde(path_selection)? {
        Some(new_path) => {
            if new_path.as_path() == current_path.as_path() {
                FzfWrapper::message("Save path unchanged.")?;
                Ok(false)
            } else {
                if let Err(err) = ensure_safe_path(new_path.as_path(), PathUsage::SaveDirectory) {
                    FzfWrapper::message(&format!("{}", err))?;
                    return Ok(false);
                }
                state.installation_mut().unwrap().save_path = new_path;
                FzfWrapper::message("Save path updated")?;
                Ok(true)
            }
        }
        None => {
            FzfWrapper::message("Save path unchanged.")?;
            Ok(false)
        }
    }
}

fn edit_optional_text_value(
    prompt: TextEditPrompt<'_>,
    current: Option<&str>,
    empty_feedback: &'static str,
    removed_feedback: &'static str,
    unchanged_feedback: &'static str,
    cancelled_feedback: &'static str,
    updated_feedback: &'static str,
    mut setter: impl FnMut(Option<String>),
) -> Result<bool> {
    match prompt_text_edit(prompt)? {
        TextEditOutcome::Cancelled => {
            FzfWrapper::message(cancelled_feedback)?;
            Ok(false)
        }
        TextEditOutcome::Unchanged => {
            FzfWrapper::message(unchanged_feedback)?;
            Ok(false)
        }
        TextEditOutcome::Updated(value) => {
            if value.is_none() && current.is_none() {
                FzfWrapper::message(empty_feedback)?;
                return Ok(false);
            }

            let is_clearing = value.is_none();
            setter(value);
            if is_clearing {
                FzfWrapper::message(removed_feedback)?;
            } else {
                FzfWrapper::message(updated_feedback)?;
            }
            Ok(true)
        }
    }
}
