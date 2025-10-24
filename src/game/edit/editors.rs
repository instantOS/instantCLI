use anyhow::{Result, anyhow};

use crate::dot::path_serde::TildePath;
use crate::menu_utils::{
    FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, PathInputBuilder, PathInputSelection,
};
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
// TODO: this function is too long, refactor
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
    let current_cmd = state.game().launch_command.as_deref().unwrap_or("");

    let new_cmd = FzfWrapper::builder()
        .prompt("Enter new launch command (leave empty to remove)")
        .header(format!(
            "Current command: {}",
            if current_cmd.is_empty() {
                "<not set>"
            } else {
                current_cmd
            }
        ))
        .input()
        .input_dialog()?;

    let trimmed = new_cmd.trim();

    if trimmed.is_empty() {
        if state.game().launch_command.is_none() {
            println!(
                "{} Launch command already empty.",
                char::from(NerdFont::Info)
            );
            return Ok(false);
        }
        state.game_mut().launch_command = None;
        println!(
            "{} Launch command removed from games.toml",
            char::from(NerdFont::Check)
        );
        return Ok(true);
    }

    if trimmed == current_cmd {
        println!("{} Launch command unchanged.", char::from(NerdFont::Info));
        return Ok(false);
    }

    state.game_mut().launch_command = Some(trimmed.to_string());
    println!(
        "{} Launch command updated in games.toml",
        char::from(NerdFont::Check)
    );
    Ok(true)
}

/// Edit the installation-specific launch command override
// TODO: get rid of duplication between this and the function above
fn edit_installation_launch_command(state: &mut EditState) -> Result<bool> {
    let installation = state
        .installation_mut()
        .ok_or_else(|| anyhow!("No installation found for this game"))?;

    let current_cmd = installation.launch_command.as_deref().unwrap_or("");

    let new_cmd = FzfWrapper::builder()
        .prompt("Enter new launch command override (leave empty to remove override)")
        .header(format!(
            "Current override: {}",
            if current_cmd.is_empty() {
                "<not set>"
            } else {
                current_cmd
            }
        ))
        .input()
        .input_dialog()?;

    let trimmed = new_cmd.trim();

    if trimmed.is_empty() {
        if installation.launch_command.is_none() {
            println!(
                "{} Launch command override already empty.",
                char::from(NerdFont::Info)
            );
            return Ok(false);
        }
        installation.launch_command = None;
        println!(
            "{} Launch command override removed from installations.toml",
            char::from(NerdFont::Check)
        );
        return Ok(true);
    }

    if trimmed == current_cmd {
        println!(
            "{} Launch command override unchanged.",
            char::from(NerdFont::Info)
        );
        return Ok(false);
    }

    installation.launch_command = Some(trimmed.to_string());
    println!(
        "{} Launch command override updated in installations.toml",
        char::from(NerdFont::Check)
    );
    Ok(true)
}

/// Edit the save path
pub fn edit_save_path(state: &mut EditState) -> Result<bool> {
    let installation = state
        .installation()
        .ok_or_else(|| anyhow!("No installation found for this game on this device"))?;

    let current_path = &installation.save_path;
    let current_path_str = current_path
        .to_tilde_string()
        .unwrap_or_else(|_| current_path.as_path().to_string_lossy().to_string());

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

    match path_selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                println!(
                    "{} No path entered. Save path unchanged.",
                    char::from(NerdFont::Warning)
                );
                return Ok(false);
            }
            let new_path =
                TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid save path: {}", e))?;

            if new_path.as_path() == current_path.as_path() {
                println!("{} Save path unchanged.", char::from(NerdFont::Info));
                return Ok(false);
            }

            state.installation_mut().unwrap().save_path = new_path;
            println!("{} Save path updated", char::from(NerdFont::Check));
            Ok(true)
        }
        PathInputSelection::Picker(path) => {
            let new_path = TildePath::new(path);

            if new_path.as_path() == current_path.as_path() {
                println!("{} Save path unchanged.", char::from(NerdFont::Info));
                return Ok(false);
            }

            state.installation_mut().unwrap().save_path = new_path;
            println!("{} Save path updated", char::from(NerdFont::Check));
            Ok(true)
        }
        PathInputSelection::Cancelled => {
            println!("{} Save path unchanged.", char::from(NerdFont::Info));
            Ok(false)
        }
    }
}

/// Launch the game
pub fn launch_game(game_name: &str) -> Result<()> {
    use crate::game::operations::launch_game;

    println!("\n{} Launching game...\n", char::from(NerdFont::Rocket));
    launch_game(Some(game_name.to_string()))
}
