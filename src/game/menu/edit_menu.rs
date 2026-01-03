use anyhow::Result;

use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use super::editors;
use super::state::EditState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Exit,
}

/// Menu action types
#[derive(Debug, Clone)]
pub enum MenuAction {
    EditName,
    EditDescription,
    EditLaunchCommand,
    EditSavePath,
    Save,
    Back,
}

/// Menu item with display text, preview, and action
#[derive(Debug, Clone)]
pub struct MenuItem {
    display: String,
    preview: String,
    action: MenuAction,
}

impl MenuItem {
    fn new(display: String, preview: String, action: MenuAction) -> Self {
        Self {
            display,
            preview,
            action,
        }
    }
}

impl FzfSelectable for MenuItem {
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

/// Run the main edit menu loop
pub fn run_edit_menu(game_name: &str, state: &mut EditState) -> Result<()> {
    let mut last_action: Option<MenuAction> = None;

    loop {
        let menu_items = build_menu_items(state);

        // Find index of last selected action to restore cursor position
        let initial_index = last_action.as_ref().and_then(|action| {
            menu_items.iter().position(|item| {
                std::mem::discriminant(&item.action) == std::mem::discriminant(action)
            })
        });

        let mut builder = FzfWrapper::builder()
            .header(format!("Editing: {}", game_name))
            .prompt("Select property to edit");

        if let Some(index) = initial_index {
            builder = builder.initial_index(index);
        }

        let selection = builder.select(menu_items)?;

        match selection {
            FzfResult::Selected(item) => {
                last_action = Some(item.action.clone());
                if handle_menu_action(item.action, state)? == Flow::Exit {
                    return Ok(());
                }
            }
            FzfResult::Cancelled => {
                if handle_cancel(state)? == Flow::Exit {
                    return Ok(());
                }
            }
            _ => return Ok(()),
        }
    }
}

fn handle_menu_action(action: MenuAction, state: &mut EditState) -> Result<Flow> {
    match action {
        MenuAction::EditName => {
            if editors::edit_name(state)? {
                state.mark_dirty();
            }
        }
        MenuAction::EditDescription => {
            if editors::edit_description(state)? {
                state.mark_dirty();
            }
        }
        MenuAction::EditLaunchCommand => {
            if editors::edit_launch_command(state)? {
                state.mark_dirty();
            }
        }
        MenuAction::EditSavePath => {
            if state.installation_index.is_some() {
                if editors::edit_save_path(state)? {
                    state.mark_dirty();
                }
            } else {
                println!(
                    "{} No installation found for this game on this device.",
                    char::from(NerdFont::Warning)
                );
            }
        }
        MenuAction::Save => {
            if state.is_dirty() {
                state.save()?;
            } else {
                println!("{} No changes to save.", char::from(NerdFont::Info));
            }
            // Stay in menu after saving
        }
        MenuAction::Back => {
            if state.is_dirty() {
                if confirm_discard_changes()? {
                    println!(
                        "{} Exited without saving changes.",
                        char::from(NerdFont::Info)
                    );
                    return Ok(Flow::Exit);
                }
            } else {
                return Ok(Flow::Exit);
            }
        }
    }
    Ok(Flow::Continue)
}

/// Handle cancel/escape from main menu
fn handle_cancel(state: &EditState) -> Result<Flow> {
    if state.is_dirty() {
        if confirm_discard_changes()? {
            println!(
                "{} Exited without saving changes.",
                char::from(NerdFont::Info)
            );
            return Ok(Flow::Exit);
        }
    } else {
        return Ok(Flow::Exit);
    }
    Ok(Flow::Continue)
}

/// Confirm discarding unsaved changes
fn confirm_discard_changes() -> Result<bool> {
    let result = FzfWrapper::builder()
        .confirm("You have unsaved changes. Exit without saving?")
        .yes_text("Exit Without Saving")
        .no_text("Go Back")
        .confirm_dialog()?;

    Ok(result == ConfirmResult::Yes)
}

/// Build menu items based on current state
fn build_menu_items(state: &EditState) -> Vec<MenuItem> {
    let game = state.game();
    let installation = state.installation();

    let mut items = Vec::new();

    // Name
    items.push(MenuItem::new(
        format!("{} Name: {}", char::from(NerdFont::Edit), game.name.0),
        format!(
            "Current name: {}\n\nEdit the game's name in games.toml",
            game.name.0
        ),
        MenuAction::EditName,
    ));

    // Description
    let desc_display = game.description.as_deref().unwrap_or("<not set>");
    items.push(MenuItem::new(
        format!(
            "{} Description: {}",
            char::from(NerdFont::Info),
            desc_display
        ),
        format!(
            "Current description: {}\n\nEdit the game's description in games.toml",
            desc_display
        ),
        MenuAction::EditDescription,
    ));

    // Launch Command
    let game_cmd = game.launch_command.as_deref();
    let inst_cmd = installation.and_then(|i| i.launch_command.as_deref());

    let (effective_cmd, cmd_source) = if let Some(cmd) = inst_cmd {
        (cmd, "installations.toml (device-specific override)")
    } else if let Some(cmd) = game_cmd {
        (cmd, "games.toml (shared)")
    } else {
        ("<not set>", "not configured")
    };

    let launch_preview = format!(
        "Effective command: {}\nSource: {}\n\n",
        effective_cmd, cmd_source
    ) + &if let Some(gcmd) = game_cmd {
        format!("games.toml: {}\n", gcmd)
    } else {
        "games.toml: <not set>\n".to_string()
    } + &if let Some(icmd) = inst_cmd {
        format!("installations.toml: {}\n", icmd)
    } else {
        "installations.toml: <not set>\n".to_string()
    } + "\nThe installation-specific command overrides the shared command if both are set.";

    items.push(MenuItem::new(
        format!(
            "{} Launch Command: {}",
            char::from(NerdFont::Rocket),
            effective_cmd
        ),
        launch_preview,
        MenuAction::EditLaunchCommand,
    ));

    // Save Path (only if installation exists)
    if let Some(inst) = installation {
        let save_path_str = inst
            .save_path
            .to_tilde_string()
            .unwrap_or_else(|_| inst.save_path.as_path().to_string_lossy().to_string());

        items.push(MenuItem::new(
            format!("{} Save Path: {}", char::from(NerdFont::Folder), save_path_str),
            format!(
                "Current save path: {}\n\nEdit the save path in installations.toml (device-specific)",
                save_path_str
            ),
            MenuAction::EditSavePath,
        ));
    }

    // Actions
    items.push(MenuItem::new(
        format!("{} Save", char::from(NerdFont::Check)),
        "Save all changes (stay in menu)".to_string(),
        MenuAction::Save,
    ));

    items.push(MenuItem::new(
        format!("{} Back", char::from(NerdFont::ArrowLeft)),
        "Return to game menu (warns if unsaved changes)".to_string(),
        MenuAction::Back,
    ));

    items
}
