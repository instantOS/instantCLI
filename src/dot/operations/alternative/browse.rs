//! Browse flow for selecting dotfiles in a directory.

use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::Config;
use crate::dot::override_config::find_all_sources;
use crate::menu_utils::{FzfResult, FzfWrapper, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::prelude::*;

use super::create_flow::run_create_flow;
use super::discovery::{DiscoveryFilter, discover_dotfiles, to_display_path};
use super::flow::{Flow, emit_cancelled};
use super::picker::BrowseMenuItem;
use super::select_flow::run_select_flow;

#[derive(Clone, Copy)]
pub(crate) enum BrowseMode {
    SelectAlternative,
    CreateAlternative,
}

pub(crate) fn run_browse_menu(dir: &Path, display: &str, mode: BrowseMode) -> Result<()> {
    let filter = match mode {
        BrowseMode::SelectAlternative => DiscoveryFilter::WithAlternatives,
        BrowseMode::CreateAlternative => DiscoveryFilter::All,
    };

    // Check once at the start
    let config = Config::load(None)?;
    let initial_dotfiles = discover_dotfiles(&config, dir, filter)?;

    if initial_dotfiles.is_empty() {
        return match mode {
            BrowseMode::CreateAlternative => {
                emit(
                    Level::Info,
                    "dot.alternative.empty",
                    &format!("No dotfiles found in {}", display.cyan()),
                    None,
                );
                Ok(())
            }
            BrowseMode::SelectAlternative => offer_create_alternative(dir, display),
        };
    }

    // Main menu loop
    let mut cursor = MenuCursor::new();
    let mut preselect: Option<String> = None;

    loop {
        let config = Config::load(None)?;
        let dotfiles = discover_dotfiles(&config, dir, filter)?;

        let action_text = match mode {
            BrowseMode::SelectAlternative => "switch source",
            BrowseMode::CreateAlternative => "create alternative",
        };

        emit(
            Level::Info,
            "dot.alternative.found",
            &format!(
                "{} Found {} dotfiles in {} (select to {})",
                char::from(NerdFont::Check),
                dotfiles.len(),
                display.cyan(),
                action_text
            ),
            None,
        );

        // Build menu
        let mut menu: Vec<BrowseMenuItem> = Vec::new();
        if matches!(mode, BrowseMode::CreateAlternative) {
            menu.push(BrowseMenuItem::PickNewFile);
        }
        menu.push(BrowseMenuItem::Cancel);
        menu.extend(dotfiles.into_iter().map(BrowseMenuItem::Dotfile));

        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select dotfile in {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu) {
            builder = builder.initial_index(index);
        }

        if let Some(q) = preselect.take() {
            builder = builder.query(q);
        }

        match builder.select(menu.clone())? {
            FzfResult::Selected(BrowseMenuItem::Dotfile(selected)) => {
                cursor.update(&BrowseMenuItem::Dotfile(selected.clone()), &menu);
                let result = match mode {
                    BrowseMode::CreateAlternative => {
                        let sources = find_all_sources(&config, &selected.target_path)?;
                        run_create_flow(&selected.target_path, &selected.display_path, &sources)?
                    }
                    BrowseMode::SelectAlternative => {
                        run_select_flow(&selected.target_path, &selected.display_path)?
                    }
                };

                match result {
                    Flow::Done => {
                        // In create mode, stay in menu to allow more operations
                        if matches!(mode, BrowseMode::CreateAlternative) {
                            preselect = Some(selected.display_path);
                            continue;
                        }
                        return Ok(());
                    }
                    Flow::Continue => continue,
                    Flow::Cancelled => return Ok(()),
                }
            }
            FzfResult::Selected(BrowseMenuItem::PickNewFile) => {
                cursor.update(&BrowseMenuItem::PickNewFile, &menu);
                if let Some(path) = pick_new_file_to_track()? {
                    let file_display = to_display_path(&path);
                    let sources = find_all_sources(&config, &path)?;
                    let create_result = run_create_flow(&path, &file_display, &sources)?;
                    if matches!(create_result, Flow::Done) {
                        preselect = Some(file_display);
                    } else if matches!(create_result, Flow::Cancelled) {
                        preselect = Some(file_display);
                    }
                }
                continue;
            }
            FzfResult::Selected(BrowseMenuItem::Cancel) => {
                cursor.update(&BrowseMenuItem::Cancel, &menu);
                emit_cancelled();
                return Ok(());
            }
            FzfResult::Cancelled => {
                emit_cancelled();
                return Ok(());
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(()),
        }
    }
}

fn offer_create_alternative(dir: &Path, display: &str) -> Result<()> {
    #[derive(Clone)]
    enum Choice {
        Create,
        Cancel,
    }

    impl std::fmt::Display for Choice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Choice::Create => write!(f, "create"),
                Choice::Cancel => write!(f, "cancel"),
            }
        }
    }

    impl crate::menu_utils::FzfSelectable for Choice {
        fn fzf_display_text(&self) -> String {
            use crate::ui::catppuccin::{colors, format_icon_colored};
            match self {
                Choice::Create => format!(
                    "{} Create new alternative...",
                    format_icon_colored(NerdFont::Plus, colors::GREEN)
                ),
                Choice::Cancel => format!(
                    "{} Cancel",
                    format_icon_colored(NerdFont::Cross, colors::OVERLAY0)
                ),
            }
        }
        fn fzf_key(&self) -> String {
            self.to_string()
        }
    }

    emit(
        Level::Info,
        "dot.alternative.none_found",
        &format!(
            "{} No dotfiles with alternatives in {}",
            char::from(NerdFont::Info),
            display.cyan()
        ),
        None,
    );

    match FzfWrapper::builder()
        .header(crate::menu_utils::Header::fancy("No alternatives found"))
        .prompt("Select action: ")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(vec![Choice::Create, Choice::Cancel])?
    {
        FzfResult::Selected(Choice::Create) => {
            run_browse_menu(dir, display, BrowseMode::CreateAlternative)
        }
        _ => {
            emit_cancelled();
            Ok(())
        }
    }
}

fn pick_new_file_to_track() -> Result<Option<std::path::PathBuf>> {
    use crate::menu_utils::{FilePickerScope, MenuWrapper};

    let home = crate::dot::sources::home_dir();

    match MenuWrapper::file_picker()
        .start_dir(&home)
        .scope(FilePickerScope::Files)
        .show_hidden(true)
        .hint("Select a file to track as a dotfile")
        .pick_one()
    {
        Ok(Some(path)) => {
            if !path.starts_with(&home) {
                FzfWrapper::message("File must be in your home directory")?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            FzfWrapper::message(&format!("File picker error: {}", e))?;
            Ok(None)
        }
    }
}
