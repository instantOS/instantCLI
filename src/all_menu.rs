use anyhow::Result;

use crate::menu_utils::{
    FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor, MenuItem,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::{dot, game, settings, video};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllMenuEntry {
    Settings,
    Dotfiles,
    Games,
    Video,
    Quit,
}

impl FzfSelectable for AllMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            AllMenuEntry::Settings => format!(
                "{} Settings",
                format_icon_colored(NerdFont::Settings, colors::BLUE)
            ),
            AllMenuEntry::Dotfiles => format!(
                "{} Dotfiles",
                format_icon_colored(NerdFont::DotFile, colors::MAUVE)
            ),
            AllMenuEntry::Games => format!(
                "{} Games",
                format_icon_colored(NerdFont::Gamepad, colors::GREEN)
            ),
            AllMenuEntry::Video => format!(
                "{} Video",
                format_icon_colored(NerdFont::Video, colors::YELLOW)
            ),
            AllMenuEntry::Quit => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            AllMenuEntry::Settings => FzfPreview::Text(
                "Open the main system settings TUI (same as `ins settings`).".to_string(),
            ),
            AllMenuEntry::Dotfiles => FzfPreview::Text(
                "Open the dotfile management menu (same as `ins dot menu`).".to_string(),
            ),
            AllMenuEntry::Games => FzfPreview::Text(
                "Open the game manager menu (same as `ins game menu`).".to_string(),
            ),
            AllMenuEntry::Video => FzfPreview::Text(
                "Open the video tools menu (same as `ins video menu`).".to_string(),
            ),
            AllMenuEntry::Quit => {
                FzfPreview::Text("Close the unified menu and return to the shell.".to_string())
            }
        }
    }
}

pub async fn run_all_menu(debug: bool) -> Result<i32> {
    let mut cursor = MenuCursor::new();

    loop {
        let entries = vec![
            MenuItem::entry(AllMenuEntry::Settings),
            MenuItem::entry(AllMenuEntry::Dotfiles),
            MenuItem::entry(AllMenuEntry::Games),
            MenuItem::entry(AllMenuEntry::Video),
            MenuItem::line(),
            MenuItem::entry(AllMenuEntry::Quit),
        ];

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("InstantCLI Menus"))
            .prompt("Select interface")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&entries) {
            builder = builder.initial_index(index);
        }

        let result = builder.select_menu(entries.clone())?;

        match result {
            FzfResult::Selected(entry) => {
                cursor.update_from_key(&entry.fzf_key());
                match entry {
                    AllMenuEntry::Settings => {
                        // Match `ins settings` default behavior (non-GUI, non-privileged)
                        settings::ui::menu::run_settings_ui(debug, false, None)?;
                    }
                    AllMenuEntry::Dotfiles => {
                        // Same as `ins dot menu`
                        dot::menu::dot_menu(debug)?;
                    }
                    AllMenuEntry::Games => {
                        // Same as `ins game menu`
                        game::menu::game_menu(None)?;
                    }
                    AllMenuEntry::Video => {
                        // Same as `ins video menu`
                        video::menu::video_menu(debug).await?;
                    }
                    AllMenuEntry::Quit => return Ok(0),
                }
            }
            FzfResult::Cancelled => return Ok(1),
            FzfResult::MultiSelected(_) | FzfResult::Error(_) => return Ok(2),
        }
    }
}
