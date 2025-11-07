use anyhow::{Result, anyhow};

use crate::game::utils::path::{path_selection_to_tilde, tilde_display_string};
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, PathInputBuilder};
use crate::ui::nerd_font::NerdFont;

use super::state::EditState;

/// Edit the game name
pub fn edit_name(state: &mut EditState) -> Result<bool> {
    let current_name = &state.game().name.0;

    let new_name = FzfWrapper::builder()
        .prompt("Enter new game name")
        .header(format!("Current name: {}", current_name))
        .input()
        .input_dialog()?;

    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        println!(
            "{} Name cannot be empty. No changes made.",
            char::from(NerdFont::Warning)
        );
        return Ok(false);
    }

    if trimmed == current_name {
        println!("{} Name unchanged.", char::from(NerdFont::Info));
        return Ok(false);
    }

    // Check for duplicates
    if state.game_config.games.iter().any(|g| g.name.0 == trimmed) {
        println!(
            "{} A game with name '{}' already exists.",
            char::from(NerdFont::Warning),
            trimmed
        );
        return Ok(false);
    }

    state.game_mut().name.0 = trimmed.to_string();
    println!(
        "{} Name updated to '{}'",
        char::from(NerdFont::Check),
        trimmed
    );
    Ok(true)
}

/// Edit the game description
pub fn edit_description(state: &mut EditState) -> Result<bool> {
    let current_desc = state.game().description.as_deref().unwrap_or("");

    let new_desc = FzfWrapper::builder()
        .prompt("Enter new description (leave empty to remove)")
        .header(format!(
            "Current description: {}",
            if current_desc.is_empty() {
                "<not set>"
            } else {
                current_desc
            }
        ))
        .input()
        .input_dialog()?;

    let trimmed = new_desc.trim();

    if trimmed.is_empty() {
        if state.game().description.is_none() {
            println!("{} Description already empty.", char::from(NerdFont::Info));
            return Ok(false);
        }
        state.game_mut().description = None;
        println!("{} Description removed", char::from(NerdFont::Check));
        return Ok(true);
    }

    if trimmed == current_desc {
        println!("{} Description unchanged.", char::from(NerdFont::Info));
        return Ok(false);
    }

    state.game_mut().description = Some(trimmed.to_string());
    println!("{} Description updated", char::from(NerdFont::Check));
    Ok(true)
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

    edit_launch_command_value(
        "Enter new launch command (leave empty to remove)",
        header,
        current,
        (NerdFont::Info, "Launch command already empty."),
        (NerdFont::Check, "Launch command removed from games.toml"),
        (NerdFont::Info, "Launch command unchanged."),
        (NerdFont::Check, "Launch command updated in games.toml"),
        |value| {
            state.game_mut().launch_command = value;
        },
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

    edit_launch_command_value(
        "Enter new launch command override (leave empty to remove override)",
        header,
        current,
        (NerdFont::Info, "Launch command override already empty."),
        (
            NerdFont::Check,
            "Launch command override removed from installations.toml",
        ),
        (NerdFont::Info, "Launch command override unchanged."),
        (
            NerdFont::Check,
            "Launch command override updated in installations.toml",
        ),
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
                println!("{} Save path unchanged.", char::from(NerdFont::Info));
                Ok(false)
            } else {
                if let Err(err) = ensure_safe_path(new_path.as_path(), PathUsage::SaveDirectory) {
                    println!("{} {}", char::from(NerdFont::CrossCircle), err);
                    return Ok(false);
                }
                state.installation_mut().unwrap().save_path = new_path;
                println!("{} Save path updated", char::from(NerdFont::Check));
                Ok(true)
            }
        }
        None => {
            println!("{} Save path unchanged.", char::from(NerdFont::Info));
            Ok(false)
        }
    }
}

fn edit_launch_command_value(
    prompt: &str,
    header: String,
    current: Option<&str>,
    empty_feedback: (NerdFont, &'static str),
    removed_feedback: (NerdFont, &'static str),
    unchanged_feedback: (NerdFont, &'static str),
    updated_feedback: (NerdFont, &'static str),
    mut setter: impl FnMut(Option<String>),
) -> Result<bool> {
    let input = FzfWrapper::builder()
        .prompt(prompt)
        .header(header)
        .input()
        .input_dialog()?;

    let trimmed = input.trim();

    if trimmed.is_empty() {
        if current.is_none() {
            println!("{} {}", char::from(empty_feedback.0), empty_feedback.1);
            return Ok(false);
        }

        setter(None);
        println!("{} {}", char::from(removed_feedback.0), removed_feedback.1);
        return Ok(true);
    }

    if current == Some(trimmed) {
        println!(
            "{} {}",
            char::from(unchanged_feedback.0),
            unchanged_feedback.1
        );
        return Ok(false);
    }

    setter(Some(trimmed.to_string()));
    println!("{} {}", char::from(updated_feedback.0), updated_feedback.1);
    Ok(true)
}

/// Launch the game
pub fn launch_game(game_name: &str) -> Result<()> {
    use crate::game::operations::launch_game;

    println!("\n{} Launching game...\n", char::from(NerdFont::Rocket));
    launch_game(Some(game_name.to_string()))
}
