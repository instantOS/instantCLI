use anyhow::{Result, anyhow};

use crate::game::utils::path::{path_selection_to_tilde, tilde_display_string};
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, PathInputBuilder, TextEditOutcome,
    TextEditPrompt, prompt_text_edit,
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

    OptionalTextEditor::new(prompt, current_desc_str, "Description", |value| {
        state.game_mut().description = value;
    })
    .run()
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

    OptionalTextEditor::new(
        TextEditPrompt::new("Launch command", current)
            .header(header)
            .ghost("Leave empty to remove"),
        current,
        "Launch command",
        |value| state.game_mut().launch_command = value,
    )
    .suffix("in games.toml")
    .run()
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

    OptionalTextEditor::new(
        TextEditPrompt::new("Launch command override", current)
            .header(header)
            .ghost("Leave empty to remove override"),
        current,
        "Launch command override",
        |value| {
            if let Some(installation) = state.installation_mut() {
                installation.launch_command = value;
            }
        },
    )
    .suffix("in installations.toml")
    .run()
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

struct OptionalTextEditor<'a, F: FnMut(Option<String>)> {
    prompt: TextEditPrompt<'a>,
    current: Option<&'a str>,
    field_name: &'a str,
    setter: F,
    suffix: Option<&'a str>,
}

impl<'a, F: FnMut(Option<String>)> OptionalTextEditor<'a, F> {
    fn new(prompt: TextEditPrompt<'a>, current: Option<&'a str>, field_name: &'a str, setter: F) -> Self {
        Self { prompt, current, field_name, setter, suffix: None }
    }

    fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = Some(suffix);
        self
    }

    fn run(mut self) -> Result<bool> {
        let field = self.field_name;
        let suffix = self.suffix.map(|s| format!(" {s}")).unwrap_or_default();

        match prompt_text_edit(self.prompt)? {
            TextEditOutcome::Cancelled => {
                FzfWrapper::message(&format!("Edit cancelled. {field} unchanged."))?;
                Ok(false)
            }
            TextEditOutcome::Unchanged => {
                FzfWrapper::message(&format!("{field} unchanged."))?;
                Ok(false)
            }
            TextEditOutcome::Updated(value) => {
                if value.is_none() && self.current.is_none() {
                    FzfWrapper::message(&format!("{field} already empty."))?;
                    return Ok(false);
                }

                let is_clearing = value.is_none();
                (self.setter)(value);
                if is_clearing {
                    FzfWrapper::message(&format!("{field} removed{suffix}"))?;
                } else {
                    FzfWrapper::message(&format!("{field} updated{suffix}"))?;
                }
                Ok(true)
            }
        }
    }
}
