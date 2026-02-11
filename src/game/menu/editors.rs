use anyhow::{Result, anyhow};

use crate::game::utils::path::{path_selection_to_tilde, tilde_display_string};
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{
    FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, Header, PathInputBuilder,
    TextEditOutcome, TextEditPrompt, prompt_text_edit,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::state::EditState;

/// How the user wants to input the launch command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchCommandInputMethod {
    Build,
    CopyFromOther,
    Remove,
    Cancel,
}

#[derive(Clone)]
struct InputMethodItem {
    display: String,
    preview: FzfPreview,
    method: LaunchCommandInputMethod,
}

impl FzfSelectable for InputMethodItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        format!("{:?}", self.method)
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

/// Info about the "other" launch command source, used to offer copying
struct OtherCommandInfo<'a> {
    command: &'a str,
    source_label: &'a str,
}

/// Show a menu to choose between manual input or command builder
fn select_launch_command_input_method(
    current: Option<&str>,
    other: Option<OtherCommandInfo<'_>>,
) -> Result<LaunchCommandInputMethod> {
    let current_display = current.unwrap_or("<not set>");

    let mut items = vec![InputMethodItem {
        display: format!(
            "{} Build launch command",
            format_icon_colored(NerdFont::Rocket, colors::MAUVE)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Rocket, "Build Launch Command")
            .text("Build a launch command interactively.")
            .blank()
            .text("Options include:")
            .bullet("Manual Entry - Type any custom command")
            .bullet("umu-run - Wine/Proton games")
            .bullet("Eden - Nintendo Switch emulator")
            .bullet("Dolphin Flatpak - GameCube/Wii")
            .bullet("PCSX2 Flatpak - PlayStation 2")
            .blank()
            .text("The builder validates files and")
            .text("generates a ready-to-use command.")
            .build(),
        method: LaunchCommandInputMethod::Build,
    }];

    if current.is_some() {
        items.push(InputMethodItem {
            display: format!(
                "{} Remove command",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Trash, "Remove Command")
                .text("Clear the current launch command.")
                .blank()
                .field("Current", current_display)
                .build(),
            method: LaunchCommandInputMethod::Remove,
        });
    }

    if let Some(ref info) = other
        && current.is_none()
    {
        items.push(InputMethodItem {
            display: format!(
                "{} Copy from {}",
                format_icon_colored(NerdFont::Clipboard, colors::GREEN),
                info.source_label,
            ),
            preview: PreviewBuilder::new()
                .header(
                    NerdFont::Clipboard,
                    &format!("Copy from {}", info.source_label),
                )
                .text(&format!(
                    "Use the {} command as a starting point.",
                    info.source_label,
                ))
                .blank()
                .field("Command", info.command)
                .blank()
                .text("The command will be copied into the editor")
                .text("so you can use it as-is or adjust it.")
                .build(),
            method: LaunchCommandInputMethod::CopyFromOther,
        });
    }

    items.push(InputMethodItem {
        display: format!("{} Cancel", format_back_icon()),
        preview: PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Cancel")
            .text("Return without making changes.")
            .build(),
        method: LaunchCommandInputMethod::Cancel,
    });

    let result = FzfWrapper::builder()
        .header(Header::fancy("How do you want to set the launch command?"))
        .prompt("Method")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_padded(items)?;

    match result {
        FzfResult::Selected(item) => Ok(item.method),
        FzfResult::Cancelled => Ok(LaunchCommandInputMethod::Cancel),
        _ => Ok(LaunchCommandInputMethod::Cancel),
    }
}

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

    let inst_cmd_owned = state.installation().and_then(|i| i.launch_command.clone());
    let other = inst_cmd_owned.as_deref().map(|cmd| OtherCommandInfo {
        command: cmd,
        source_label: "device-specific override",
    });

    match select_launch_command_input_method(current, other)? {
        LaunchCommandInputMethod::Build => {
            match crate::game::launch_builder::build_launch_command()? {
                Some(command) => {
                    state.game_mut().launch_command = Some(command.clone());
                    FzfWrapper::message(&format!(
                        "{} Launch command set in games.toml:\n\n{}",
                        char::from(NerdFont::Check),
                        command
                    ))?;
                    Ok(true)
                }
                None => Ok(false),
            }
        }
        LaunchCommandInputMethod::CopyFromOther => {
            let source = inst_cmd_owned.as_deref();
            let header = format!(
                "Copied from device-specific override: {}",
                source.unwrap_or("<not set>")
            );
            OptionalTextEditor::new(
                TextEditPrompt::new("Launch command", source)
                    .header(header)
                    .ghost("Leave empty to cancel"),
                None,
                "Launch command",
                |value| state.game_mut().launch_command = value,
            )
            .suffix("in games.toml")
            .run()
        }
        LaunchCommandInputMethod::Remove => {
            // Ask for confirmation before removing
            match FzfWrapper::builder()
                .confirm(format!(
                    "{} Remove launch command from games.toml?\n\nThis will remove the shared command used across all devices.",
                    char::from(NerdFont::Trash)
                ))
                .yes_text("Remove Command")
                .no_text("Keep Command")
                .confirm_dialog()?
            {
                crate::menu_utils::ConfirmResult::Yes => {
                    state.game_mut().launch_command = None;
                    FzfWrapper::message("Launch command removed from games.toml")?;
                    Ok(true)
                }
                crate::menu_utils::ConfirmResult::No | crate::menu_utils::ConfirmResult::Cancelled => Ok(false),
            }
        }
        LaunchCommandInputMethod::Cancel => Ok(false),
    }
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

    let game_cmd_owned = state.game().launch_command.clone();
    let other = game_cmd_owned.as_deref().map(|cmd| OtherCommandInfo {
        command: cmd,
        source_label: "shared command",
    });

    match select_launch_command_input_method(current, other)? {
        LaunchCommandInputMethod::Build => {
            match crate::game::launch_builder::build_launch_command()? {
                Some(command) => {
                    if let Some(installation) = state.installation_mut() {
                        installation.launch_command = Some(command.clone());
                    }
                    FzfWrapper::message(&format!(
                        "{} Launch command override set in installations.toml:\n\n{}",
                        char::from(NerdFont::Check),
                        command
                    ))?;
                    Ok(true)
                }
                None => Ok(false),
            }
        }
        LaunchCommandInputMethod::CopyFromOther => {
            let source = game_cmd_owned.as_deref();
            let header = format!(
                "Copied from shared command: {}",
                source.unwrap_or("<not set>")
            );
            OptionalTextEditor::new(
                TextEditPrompt::new("Launch command override", source)
                    .header(header)
                    .ghost("Leave empty to cancel"),
                None,
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
        LaunchCommandInputMethod::Remove => {
            // Ask for confirmation before removing
            match FzfWrapper::builder()
                .confirm(format!(
                    "{} Remove launch command override from installations.toml?\n\nThis will remove the device-specific override.",
                    char::from(NerdFont::Trash)
                ))
                .yes_text("Remove Override")
                .no_text("Keep Override")
                .confirm_dialog()?
            {
                crate::menu_utils::ConfirmResult::Yes => {
                    if let Some(installation) = state.installation_mut() {
                        installation.launch_command = None;
                    }
                    FzfWrapper::message("Launch command override removed from installations.toml")?;
                    Ok(true)
                }
                crate::menu_utils::ConfirmResult::No | crate::menu_utils::ConfirmResult::Cancelled => Ok(false),
            }
        }
        LaunchCommandInputMethod::Cancel => Ok(false),
    }
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
    fn new(
        prompt: TextEditPrompt<'a>,
        current: Option<&'a str>,
        field_name: &'a str,
        setter: F,
    ) -> Self {
        Self {
            prompt,
            current,
            field_name,
            setter,
            suffix: None,
        }
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
