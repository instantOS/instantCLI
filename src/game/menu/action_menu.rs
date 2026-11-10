use crate::game::operations::{desktop, steam};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::state::GameState;

/// Game action selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GameAction {
    Launch,
    Edit,
    OpenSaveDirectory,
    Setup,
    Move,
    Checkpoint,
    AddToSteam,
    AddToDesktop,
    Back,
}

impl std::fmt::Display for GameAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameAction::Launch => write!(f, "launch"),
            GameAction::Edit => write!(f, "edit"),
            GameAction::OpenSaveDirectory => write!(f, "open-save-directory"),
            GameAction::Setup => write!(f, "setup"),
            GameAction::Move => write!(f, "move"),
            GameAction::Checkpoint => write!(f, "checkpoint"),
            GameAction::AddToSteam => write!(f, "add-to-steam"),
            GameAction::AddToDesktop => write!(f, "add-to-desktop"),
            GameAction::Back => write!(f, "back"),
        }
    }
}

#[derive(Clone)]
pub(super) struct GameActionItem {
    display: String,
    pub(super) action: GameAction,
    preview: FzfPreview,
    keywords: Vec<&'static str>,
}

impl FzfSelectable for GameActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.action.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_search_keywords(&self) -> &[&str] {
        &self.keywords
    }
}

/// Build the game action menu items
pub(super) fn build_action_menu(game_name: &str, state: &GameState) -> Vec<GameActionItem> {
    let mut actions: Vec<GameActionItem> = Vec::new();

    if state.needs_setup {
        actions.push(action_setup_item(game_name));
    }

    actions.push(action_launch_item(game_name, state));
    actions.push(action_edit_item(game_name, state));

    if !state.needs_setup {
        if let Some(path) = &state.save_path {
            actions.push(action_open_save_item(game_name, path));
            actions.push(action_move_item(game_name, path));
        }
        actions.push(action_checkpoint_item(game_name));
    }

    if state.launch_command.is_some() {
        actions.push(action_steam_item(game_name));
        actions.push(action_desktop_item(game_name));
    }

    actions.push(action_back_item());
    actions
}

fn action_setup_item(game_name: &str) -> GameActionItem {
    GameActionItem {
        display: format!(
            "{} Setup",
            format_icon_colored(NerdFont::Wrench, colors::PEACH)
        ),
        action: GameAction::Setup,
        preview: PreviewBuilder::new()
            .header(NerdFont::Wrench, "Setup Game")
            .text(&format!(
                "Configure save path for '{}' on this device.",
                game_name
            ))
            .blank()
            .text("This game is registered but has no save data location set up yet.")
            .build(),
        keywords: vec![],
    }
}

fn action_launch_item(game_name: &str, state: &GameState) -> GameActionItem {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Rocket, "Launch Game")
        .text(&format!("Launch {} with automatic save sync.", game_name));

    if let Some(cmd) = &state.launch_command {
        builder = builder.blank().field("Command", cmd);
    } else {
        builder = builder
            .blank()
            .line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "No launch command configured.",
            )
            .subtext("Pressing Launch will offer to build one.");
    }

    GameActionItem {
        display: format!(
            "{} Launch",
            format_icon_colored(NerdFont::Rocket, colors::GREEN)
        ),
        action: GameAction::Launch,
        preview: builder.build(),
        keywords: vec!["play", "start", "run"],
    }
}

fn action_edit_item(game_name: &str, state: &GameState) -> GameActionItem {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Edit, "Edit Configuration")
        .text(&format!("Edit configuration for '{}'", game_name))
        .blank()
        .separator()
        .blank()
        .line(colors::MAUVE, Some(NerdFont::FileConfig), "games.toml")
        .field_indented(
            "Description",
            state.description.as_deref().unwrap_or("<not set>"),
        )
        .field_indented(
            "Launch Command",
            state.launch_command.as_deref().unwrap_or("<not set>"),
        );

    if let Some(path) = &state.save_path {
        builder = builder
            .blank()
            .line(colors::MAUVE, Some(NerdFont::Desktop), "installations.toml")
            .field_indented("Save Path", path);
    } else {
        builder = builder.blank().subtext("No installation on this device");
    }

    GameActionItem {
        display: format!(
            "{} Edit",
            format_icon_colored(NerdFont::Edit, colors::SAPPHIRE)
        ),
        action: GameAction::Edit,
        preview: builder.build(),
        keywords: vec!["change", "modify", "configure", "settings"],
    }
}

fn action_open_save_item(game_name: &str, path: &str) -> GameActionItem {
    GameActionItem {
        display: format!(
            "{} Open Save Directory",
            format_icon_colored(NerdFont::FolderOpen, colors::GREEN)
        ),
        action: GameAction::OpenSaveDirectory,
        preview: PreviewBuilder::new()
            .header(NerdFont::FolderOpen, "Open Save Directory")
            .text(&format!("Open the save location for '{}'.", game_name))
            .blank()
            .field("Configured path", path)
            .blank()
            .text("Choose how to open it:")
            .bullet("yazi file manager")
            .bullet("Terminal window in that directory")
            .bullet("Default desktop application via xdg-open")
            .build(),
        keywords: vec!["folder", "browse", "explore", "files"],
    }
}

fn action_move_item(game_name: &str, path: &str) -> GameActionItem {
    GameActionItem {
        display: format!(
            "{} Change Save Path",
            format_icon_colored(NerdFont::Folder, colors::LAVENDER)
        ),
        action: GameAction::Move,
        preview: PreviewBuilder::new()
            .header(NerdFont::Folder, "Change Save Path")
            .text(&format!("Select a new save location for '{}'.", game_name))
            .blank()
            .subtext("This updates where the game manager looks for saves.")
            .subtext("Files are not moved automatically.")
            .blank()
            .field("Current path", path)
            .build(),
        keywords: vec![],
    }
}

fn action_checkpoint_item(game_name: &str) -> GameActionItem {
    GameActionItem {
        display: format!(
            "{} Checkpoint",
            format_icon_colored(NerdFont::Clock, colors::YELLOW)
        ),
        action: GameAction::Checkpoint,
        preview: PreviewBuilder::new()
            .header(NerdFont::Clock, "Restore Checkpoint")
            .text(&format!(
                "Browse and restore from a past checkpoint for '{}'.",
                game_name
            ))
            .blank()
            .text("View all available snapshots and select one to restore.")
            .text("If local saves are newer, you will be warned before overwriting.")
            .build(),
        keywords: vec!["snapshot"],
    }
}

fn action_steam_item(game_name: &str) -> GameActionItem {
    let is_in_steam = steam::is_game_in_steam(game_name).unwrap_or(false);
    let status_text = if is_in_steam {
        format!(
            "{} Already in Steam Library",
            format_icon_colored(NerdFont::Check, colors::GREEN)
        )
    } else {
        format!(
            "{} Not in Steam Library",
            format_icon_colored(NerdFont::Cross, colors::RED)
        )
    };

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Steam, "Add to Steam")
        .text(&format!("Manage Steam shortcut for '{}'.", game_name))
        .blank()
        .text(&status_text);

    if is_in_steam && let Ok(Some(shortcut)) = steam::get_game_shortcut(game_name) {
        builder = builder.blank().text("Current shortcut details:");
        builder = builder
            .subtext(&format!("Exe:        {}", shortcut.exe))
            .subtext(&format!("Start dir:  {}", shortcut.start_dir));

        if !shortcut.launch_options.is_empty() {
            builder = builder.subtext(&format!("Options:    {}", shortcut.launch_options));
        }

        let full_cmd = if shortcut.launch_options.is_empty() {
            shortcut.exe.clone()
        } else {
            format!("{} {}", shortcut.exe, shortcut.launch_options)
        };
        builder = builder
            .blank()
            .subtext(&format!("Full command: {}", full_cmd));
    }

    let preview = builder
        .blank()
        .text("The shortcut will launch the game through ins,")
        .text("which automatically syncs saves before and after.")
        .blank()
        .subtext("Note: Restart Steam after adding to see the shortcut.")
        .build();

    GameActionItem {
        display: format!(
            "{} Add to Steam",
            format_icon_colored(NerdFont::Steam, colors::SAPPHIRE)
        ),
        action: GameAction::AddToSteam,
        preview,
        keywords: vec![],
    }
}

fn action_desktop_item(game_name: &str) -> GameActionItem {
    let is_on_desktop = desktop::is_game_on_desktop(game_name).unwrap_or(false);
    let status_text = if is_on_desktop {
        format!(
            "{} Already on Desktop",
            format_icon_colored(NerdFont::Check, colors::GREEN)
        )
    } else {
        format!(
            "{} Not on Desktop",
            format_icon_colored(NerdFont::Cross, colors::RED)
        )
    };

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Desktop, "Add to Desktop")
        .text(&format!("Manage desktop shortcut for '{}'.", game_name))
        .blank()
        .text(&status_text);

    if is_on_desktop && let Ok(Some(path)) = desktop::get_game_desktop_path(game_name) {
        builder = builder
            .blank()
            .text("Current shortcut location:")
            .subtext(&path.display().to_string());
    }

    let preview = builder
        .blank()
        .text("Creates a .desktop file that launches the game through ins,")
        .text("which automatically syncs saves before and after.")
        .blank()
        .subtext("The shortcut will be placed on your Desktop if possible,")
        .subtext("otherwise in the applications menu.")
        .build();

    GameActionItem {
        display: format!(
            "{} Add to Desktop",
            format_icon_colored(NerdFont::Desktop, colors::MAUVE)
        ),
        action: GameAction::AddToDesktop,
        preview,
        keywords: vec!["shortcut", "launcher"],
    }
}

fn action_back_item() -> GameActionItem {
    GameActionItem {
        display: format!("{} Back", format_back_icon()),
        action: GameAction::Back,
        preview: PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to game selection.")
            .build(),
        keywords: vec![],
    }
}
