use anyhow::{Context, Result, anyhow};

use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::game::games::selection::select_game_interactive;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, PathInputBuilder, PathInputSelection};
use crate::ui::nerd_font::NerdFont;
use crate::dot::path_serde::TildePath;

/// Main entry point for editing a game
pub fn edit_game(game_name: Option<String>) -> Result<()> {
    let game_name = match game_name {
        Some(name) => name,
        None => match select_game_interactive(None)? {
            Some(name) => name,
            None => return Ok(()),
        },
    };

    // Load both configs
    let mut game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations = InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Find the game and installation
    let game_index = game_config
        .games
        .iter()
        .position(|g| g.name.0 == game_name)
        .ok_or_else(|| anyhow!("Game '{}' not found in games.toml", game_name))?;

    let installation_index = installations
        .installations
        .iter()
        .position(|i| i.game_name.0 == game_name);

    // Run the edit menu loop
    edit_game_menu(
        &game_name,
        &mut game_config,
        &mut installations,
        game_index,
        installation_index,
    )
}

/// Interactive menu for editing a game
fn edit_game_menu(
    game_name: &str,
    game_config: &mut InstantGameConfig,
    installations: &mut InstallationsConfig,
    game_index: usize,
    installation_index: Option<usize>,
) -> Result<()> {
    let mut dirty = false;

    loop {
        let game = &game_config.games[game_index];
        let installation = installation_index.map(|idx| &installations.installations[idx]);

        let menu_items = build_menu_items(game, installation);

        let selection = FzfWrapper::builder()
            .header(format!("Editing: {}", game_name))
            .prompt("Select property to edit")
            .select(menu_items)?;

        match selection {
            FzfResult::Selected(item) => {
                match item.action {
                    MenuAction::EditName => {
                        if edit_name(game_config, game_index)? {
                            dirty = true;
                        }
                    }
                    MenuAction::EditDescription => {
                        if edit_description(game_config, game_index)? {
                            dirty = true;
                        }
                    }
                    MenuAction::EditLaunchCommand => {
                        if edit_launch_command(game_config, installations, game_index, installation_index)? {
                            dirty = true;
                        }
                    }
                    MenuAction::EditSavePath => {
                        if let Some(inst_idx) = installation_index {
                            if edit_save_path(installations, inst_idx)? {
                                dirty = true;
                            }
                        } else {
                            println!("{} No installation found for this game on this device.", char::from(NerdFont::Warning));
                        }
                    }
                    MenuAction::LaunchGame => {
                        // Save before launching
                        if dirty {
                            save_configs(game_config, installations)?;
                            dirty = false;
                        }
                        launch_game_from_edit(game_name)?;
                    }
                    MenuAction::SaveAndExit => {
                        if dirty {
                            save_configs(game_config, installations)?;
                            println!("{} Changes saved successfully.", char::from(NerdFont::Check));
                        }
                        return Ok(());
                    }
                    MenuAction::ExitWithoutSaving => {
                        if dirty {
                            let confirm = FzfWrapper::builder()
                                .confirm("You have unsaved changes. Exit without saving?")
                                .yes_text("Exit Without Saving")
                                .no_text("Go Back")
                                .confirm_dialog()?;

                            if confirm == crate::menu_utils::ConfirmResult::Yes {
                                println!("{} Exited without saving changes.", char::from(NerdFont::Info));
                                return Ok(());
                            }
                        } else {
                            return Ok(());
                        }
                    }
                }
            }
            FzfResult::Cancelled => {
                if dirty {
                    let confirm = FzfWrapper::builder()
                        .confirm("You have unsaved changes. Exit without saving?")
                        .yes_text("Exit Without Saving")
                        .no_text("Go Back")
                        .confirm_dialog()?;

                    if confirm == crate::menu_utils::ConfirmResult::Yes {
                        println!("{} Exited without saving changes.", char::from(NerdFont::Info));
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
            _ => return Ok(()),
        }
    }
}

#[derive(Debug, Clone)]
enum MenuAction {
    EditName,
    EditDescription,
    EditLaunchCommand,
    EditSavePath,
    LaunchGame,
    SaveAndExit,
    ExitWithoutSaving,
}

#[derive(Debug, Clone)]
struct MenuItem {
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

fn build_menu_items(game: &Game, installation: Option<&GameInstallation>) -> Vec<MenuItem> {
    let mut items = Vec::new();

    // Name
    items.push(MenuItem::new(
        format!("{} Name: {}", char::from(NerdFont::Edit), game.name.0),
        format!("Current name: {}\n\nEdit the game's name in games.toml", game.name.0),
        MenuAction::EditName,
    ));

    // Description
    let desc_display = game.description.as_deref().unwrap_or("<not set>");
    items.push(MenuItem::new(
        format!("{} Description: {}", char::from(NerdFont::Info), desc_display),
        format!(
            "Current description: {}\n\nEdit the game's description in games.toml",
            desc_display
        ),
        MenuAction::EditDescription,
    ));

    // Launch Command - show both sources
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
        format!("{} Launch Command: {}", char::from(NerdFont::Rocket), effective_cmd),
        launch_preview,
        MenuAction::EditLaunchCommand,
    ));

    // Save Path (only if installation exists)
    if let Some(inst) = installation {
        let save_path_str = inst.save_path.to_tilde_string()
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
        format!("{} Launch Game", char::from(NerdFont::Rocket)),
        "Launch the game (saves changes first)".to_string(),
        MenuAction::LaunchGame,
    ));

    items.push(MenuItem::new(
        format!("{} Save and Exit", char::from(NerdFont::Check)),
        "Save all changes and exit".to_string(),
        MenuAction::SaveAndExit,
    ));

    items.push(MenuItem::new(
        format!("{} Exit Without Saving", char::from(NerdFont::Cross)),
        "Discard all changes and exit".to_string(),
        MenuAction::ExitWithoutSaving,
    ));

    items
}

fn edit_name(game_config: &mut InstantGameConfig, game_index: usize) -> Result<bool> {
    let current_name = &game_config.games[game_index].name.0;
    
    let new_name = FzfWrapper::builder()
        .prompt("Enter new game name")
        .header(format!("Current name: {}", current_name))
        .input()
        .input_dialog()?;

    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        println!("{} Name cannot be empty. No changes made.", char::from(NerdFont::Warning));
        return Ok(false);
    }

    if trimmed == current_name {
        println!("{} Name unchanged.", char::from(NerdFont::Info));
        return Ok(false);
    }

    // Check for duplicates
    if game_config.games.iter().any(|g| g.name.0 == trimmed) {
        println!("{} A game with name '{}' already exists.", char::from(NerdFont::Warning), trimmed);
        return Ok(false);
    }

    game_config.games[game_index].name.0 = trimmed.to_string();
    println!("{} Name updated to '{}'", char::from(NerdFont::Check), trimmed);
    Ok(true)
}

fn edit_description(game_config: &mut InstantGameConfig, game_index: usize) -> Result<bool> {
    let current_desc = game_config.games[game_index].description.as_deref().unwrap_or("");
    
    let new_desc = FzfWrapper::builder()
        .prompt("Enter new description (leave empty to remove)")
        .header(format!("Current description: {}", if current_desc.is_empty() { "<not set>" } else { current_desc }))
        .input()
        .input_dialog()?;

    let trimmed = new_desc.trim();
    
    if trimmed.is_empty() {
        if game_config.games[game_index].description.is_none() {
            println!("{} Description already empty.", char::from(NerdFont::Info));
            return Ok(false);
        }
        game_config.games[game_index].description = None;
        println!("{} Description removed", char::from(NerdFont::Check));
        return Ok(true);
    }

    if trimmed == current_desc {
        println!("{} Description unchanged.", char::from(NerdFont::Info));
        return Ok(false);
    }

    game_config.games[game_index].description = Some(trimmed.to_string());
    println!("{} Description updated", char::from(NerdFont::Check));
    Ok(true)
}

fn edit_launch_command(
    game_config: &mut InstantGameConfig,
    installations: &mut InstallationsConfig,
    game_index: usize,
    installation_index: Option<usize>,
) -> Result<bool> {
    let game = &game_config.games[game_index];
    let installation = installation_index.map(|idx| &installations.installations[idx]);

    let game_cmd = game.launch_command.as_deref();
    let inst_cmd = installation.and_then(|i| i.launch_command.as_deref());

    // Build menu to choose which to edit
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

    let mut options = vec![
        LaunchCommandOption {
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
        },
    ];

    if installation_index.is_some() {
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
            LaunchCommandTarget::GameConfig => {
                edit_game_launch_command(game_config, game_index)
            }
            LaunchCommandTarget::Installation => {
                if let Some(idx) = installation_index {
                    edit_installation_launch_command(installations, idx)
                } else {
                    Ok(false)
                }
            }
            LaunchCommandTarget::Back => Ok(false),
        },
        _ => Ok(false),
    }
}

fn edit_game_launch_command(game_config: &mut InstantGameConfig, game_index: usize) -> Result<bool> {
    let current_cmd = game_config.games[game_index].launch_command.as_deref().unwrap_or("");

    let new_cmd = FzfWrapper::builder()
        .prompt("Enter new launch command (leave empty to remove)")
        .header(format!("Current command: {}", if current_cmd.is_empty() { "<not set>" } else { current_cmd }))
        .input()
        .input_dialog()?;

    let trimmed = new_cmd.trim();

    if trimmed.is_empty() {
        if game_config.games[game_index].launch_command.is_none() {
            println!("{} Launch command already empty.", char::from(NerdFont::Info));
            return Ok(false);
        }
        game_config.games[game_index].launch_command = None;
        println!("{} Launch command removed from games.toml", char::from(NerdFont::Check));
        return Ok(true);
    }

    if trimmed == current_cmd {
        println!("{} Launch command unchanged.", char::from(NerdFont::Info));
        return Ok(false);
    }

    game_config.games[game_index].launch_command = Some(trimmed.to_string());
    println!("{} Launch command updated in games.toml", char::from(NerdFont::Check));
    Ok(true)
}

fn edit_installation_launch_command(installations: &mut InstallationsConfig, installation_index: usize) -> Result<bool> {
    let current_cmd = installations.installations[installation_index].launch_command.as_deref().unwrap_or("");

    let new_cmd = FzfWrapper::builder()
        .prompt("Enter new launch command override (leave empty to remove override)")
        .header(format!("Current override: {}", if current_cmd.is_empty() { "<not set>" } else { current_cmd }))
        .input()
        .input_dialog()?;

    let trimmed = new_cmd.trim();

    if trimmed.is_empty() {
        if installations.installations[installation_index].launch_command.is_none() {
            println!("{} Launch command override already empty.", char::from(NerdFont::Info));
            return Ok(false);
        }
        installations.installations[installation_index].launch_command = None;
        println!("{} Launch command override removed from installations.toml", char::from(NerdFont::Check));
        return Ok(true);
    }

    if trimmed == current_cmd {
        println!("{} Launch command override unchanged.", char::from(NerdFont::Info));
        return Ok(false);
    }

    installations.installations[installation_index].launch_command = Some(trimmed.to_string());
    println!("{} Launch command override updated in installations.toml", char::from(NerdFont::Check));
    Ok(true)
}

fn edit_save_path(installations: &mut InstallationsConfig, installation_index: usize) -> Result<bool> {
    let current_path = &installations.installations[installation_index].save_path;
    let current_path_str = current_path.to_tilde_string()
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
        .scope(crate::menu_utils::FilePickerScope::Directories)
        .picker_hint(format!(
            "{} Select the directory to use for save files",
            char::from(NerdFont::Info)
        ))
        .manual_option_label(format!("{} Type an exact path", char::from(NerdFont::Edit)))
        .picker_option_label(format!(
            "{} Browse and choose a folder",
            char::from(NerdFont::FolderOpen)
        ))
        .choose()?;

    match path_selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                println!("{} No path entered. Save path unchanged.", char::from(NerdFont::Warning));
                return Ok(false);
            }
            let new_path = TildePath::from_str(trimmed)
                .map_err(|e| anyhow!("Invalid save path: {}", e))?;

            if new_path.as_path() == current_path.as_path() {
                println!("{} Save path unchanged.", char::from(NerdFont::Info));
                return Ok(false);
            }

            installations.installations[installation_index].save_path = new_path;
            println!("{} Save path updated", char::from(NerdFont::Check));
            Ok(true)
        }
        PathInputSelection::Picker(path) => {
            let new_path = TildePath::new(path);

            if new_path.as_path() == current_path.as_path() {
                println!("{} Save path unchanged.", char::from(NerdFont::Info));
                return Ok(false);
            }

            installations.installations[installation_index].save_path = new_path;
            println!("{} Save path updated", char::from(NerdFont::Check));
            Ok(true)
        }
        PathInputSelection::Cancelled => {
            println!("{} Save path unchanged.", char::from(NerdFont::Info));
            Ok(false)
        }
    }
}

fn save_configs(game_config: &InstantGameConfig, installations: &InstallationsConfig) -> Result<()> {
    game_config.save().context("Failed to save games.toml")?;
    installations.save().context("Failed to save installations.toml")?;
    Ok(())
}

fn launch_game_from_edit(game_name: &str) -> Result<()> {
    use crate::game::operations::launch_game;

    println!("\n{} Launching game...\n", char::from(NerdFont::Rocket));
    launch_game(Some(game_name.to_string()))
}

