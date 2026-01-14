//! Interactive dot menu for managing dotfile repositories

mod add_repo;
mod repo_actions;
mod subdir_actions;

use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use add_repo::handle_add_repo;
use repo_actions::{build_repo_preview, handle_repo_actions};

/// Menu entry for dotfile main menu
#[derive(Debug, Clone)]
pub enum DotMenuEntry {
    Repo(String),
    AddRepo,
    AlternateFiles,
    CloseMenu,
}

impl FzfSelectable for DotMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            DotMenuEntry::Repo(name) => {
                format!(
                    "{} {}",
                    format_icon_colored(NerdFont::Folder, colors::MAUVE),
                    name
                )
            }
            DotMenuEntry::AddRepo => {
                format!(
                    "{} Add Repo",
                    format_icon_colored(NerdFont::Plus, colors::GREEN)
                )
            }
            DotMenuEntry::AlternateFiles => {
                format!(
                    "{} Alternate Files",
                    format_icon_colored(NerdFont::List, colors::PEACH)
                )
            }
            DotMenuEntry::CloseMenu => format!("{} Close Menu", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            DotMenuEntry::Repo(name) => name.clone(),
            DotMenuEntry::AddRepo => "!__add_repo__".to_string(),
            DotMenuEntry::AlternateFiles => "!__alternate_files__".to_string(),
            DotMenuEntry::CloseMenu => "!__close_menu__".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;

        match self {
            DotMenuEntry::Repo(_) => FzfPreview::Text("Repository information".to_string()),
            DotMenuEntry::AddRepo => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Repository")
                .text("Clone a new dotfile repository")
                .blank()
                .text("This will:")
                .bullet("Prompt for repository URL")
                .bullet("Clone the repository")
                .bullet("Apply dotfiles from the new repo")
                .build(),
            DotMenuEntry::AlternateFiles => PreviewBuilder::new()
                .header(NerdFont::List, "Alternate Files")
                .text("Browse dotfiles with multiple sources")
                .blank()
                .text("Select which repository or subdirectory")
                .text("a dotfile should be sourced from when")
                .text("multiple versions exist.")
                .build(),
            DotMenuEntry::CloseMenu => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close Menu")
                .text("Close the dotfile menu")
                .blank()
                .text("This will exit the interactive menu")
                .text("and return to the command prompt")
                .build(),
        }
    }
}

/// Wrapper struct for menu items with custom previews
#[derive(Clone)]
struct DotMenuItem {
    entry: DotMenuEntry,
    preview: String,
}

impl FzfSelectable for DotMenuItem {
    fn fzf_display_text(&self) -> String {
        self.entry.fzf_display_text()
    }

    fn fzf_key(&self) -> String {
        self.entry.fzf_key()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

/// Select a menu entry from the main dot menu
fn select_dot_menu_entry(config: &Config, db: &Database) -> Result<Option<DotMenuEntry>> {
    let mut entries: Vec<DotMenuEntry> = config
        .repos
        .iter()
        .map(|r| DotMenuEntry::Repo(r.name.clone()))
        .collect();

    entries.push(DotMenuEntry::AddRepo);
    entries.push(DotMenuEntry::AlternateFiles);
    entries.push(DotMenuEntry::CloseMenu);

    // Create entries with custom previews (Repo gets dynamic preview, others use trait impl)
    let menu_items: Vec<DotMenuItem> = entries
        .into_iter()
        .map(|entry| {
            let preview = match &entry {
                DotMenuEntry::Repo(name) => build_repo_preview(name, config, db),
                _ => match entry.fzf_preview() {
                    crate::menu::protocol::FzfPreview::Text(s) => s,
                    crate::menu::protocol::FzfPreview::Command(s) => s,
                    crate::menu::protocol::FzfPreview::None => String::new(),
                },
            };
            DotMenuItem { entry, preview }
        })
        .collect();

    let result = FzfWrapper::builder()
        .header(Header::fancy("Dotfile Menu"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(menu_items)?;

    match result {
        FzfResult::Selected(item) => Ok(Some(item.entry)),
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

/// Main entry point for the dot menu
pub fn dot_menu(debug: bool) -> Result<()> {
    // Outer loop: main menu
    loop {
        // Load config each iteration to pick up changes (e.g., newly added repos)
        let config = Config::load(None)?;
        let db = Database::new(config.database_path().to_path_buf())?;

        let entry = match select_dot_menu_entry(&config, &db)? {
            Some(entry) => entry,
            None => return Ok(()),
        };

        match entry {
            DotMenuEntry::Repo(repo_name) => {
                handle_repo_actions(&repo_name, &config, &db, debug)?;
            }
            DotMenuEntry::AddRepo => {
                handle_add_repo(&config, &db, debug)?;
            }
            DotMenuEntry::AlternateFiles => {
                crate::dot::operations::alternative::handle_alternative(
                    &config, "~", false, false, false,
                )?;
            }
            DotMenuEntry::CloseMenu => return Ok(()),
        }
    }
}
