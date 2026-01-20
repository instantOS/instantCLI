//! Create flow for adding alternatives.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::override_config::{DotfileSource, OverrideConfig, find_all_sources};
use crate::menu_utils::{FzfResult, FzfWrapper, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::prelude::*;

use super::apply::add_to_destination;
use super::discovery::{get_destinations, to_display_path};
use super::flow::{Flow, emit_cancelled, message_and_continue, message_and_done};
use super::picker::{CreateMenuItem, SourceOption};

/// Pick a destination and add a file there (shared by `add --choose` and `alternative --create`).
pub fn pick_destination_and_add(config: &Config, path: &Path) -> Result<bool> {
    let display = to_display_path(path);
    let existing = find_all_sources(config, path)?;
    match run_create_flow(path, &display, &existing)? {
        Flow::Done => Ok(true),
        _ => Ok(false),
    }
}

// Create flow - adding a file to a new destination.
pub(crate) fn run_create_flow(
    path: &Path,
    display: &str,
    existing: &[DotfileSource],
) -> Result<Flow> {
    let mut cursor = MenuCursor::new();

    loop {
        let config = Config::load(None)?;
        let destinations = get_destinations(&config);

        // Build menu
        let mut menu: Vec<CreateMenuItem> = destinations
            .iter()
            .map(|dest| {
                let exists = existing
                    .iter()
                    .any(|s| s.repo_name == dest.repo_name && s.subdir_name == dest.subdir_name);
                CreateMenuItem::Destination(SourceOption {
                    source: dest.clone(),
                    is_current: false,
                    exists,
                })
            })
            .collect();

        // Add "new subdir" options
        let repos_with_subdirs: HashSet<_> = destinations.iter().map(|d| &d.repo_name).collect();
        for repo in config.repos.iter().filter(|r| r.enabled && !r.read_only) {
            if repos_with_subdirs.contains(&repo.name) {
                menu.push(CreateMenuItem::AddSubdir {
                    repo_name: repo.name.clone(),
                });
            }
        }

        menu.push(CreateMenuItem::CloneRepo);
        menu.push(CreateMenuItem::Cancel);

        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select destination for {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu) {
            builder = builder.initial_index(index);
        }

        match builder.select(menu.clone())? {
            FzfResult::Selected(CreateMenuItem::Destination(item)) => {
                cursor.update(&CreateMenuItem::Destination(item.clone()), &menu);
                match add_file_to_destination(&config, path, display, &item)? {
                    Flow::Continue => continue,
                    other => return Ok(other),
                }
            }
            FzfResult::Selected(CreateMenuItem::AddSubdir { repo_name }) => {
                cursor.update(
                    &CreateMenuItem::AddSubdir {
                        repo_name: repo_name.clone(),
                    },
                    &menu,
                );
                if create_new_subdir(&config, &repo_name)? {
                    continue;
                }
                return Ok(Flow::Cancelled);
            }
            FzfResult::Selected(CreateMenuItem::CloneRepo) => {
                cursor.update(&CreateMenuItem::CloneRepo, &menu);
                if clone_new_repo()? {
                    continue;
                }
                return Ok(Flow::Cancelled);
            }
            FzfResult::Selected(CreateMenuItem::Cancel) => {
                cursor.update(&CreateMenuItem::Cancel, &menu);
                emit_cancelled();
                return Ok(Flow::Cancelled);
            }
            FzfResult::Cancelled => {
                emit_cancelled();
                return Ok(Flow::Cancelled);
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(Flow::Cancelled),
        }
    }
}

fn add_file_to_destination(
    config: &Config,
    path: &Path,
    display: &str,
    item: &SourceOption,
) -> Result<Flow> {
    // Already exists at this destination
    if item.exists {
        return message_and_continue(&format!(
            "'{}' already exists at {} / {}\n\n\
            This location is already tracked as an alternative.\n\
            Use the alternative selection menu to switch sources.",
            display, item.source.repo_name, item.source.subdir_name
        ));
    }

    // Open database
    let db = match Database::new(config.database_path().to_path_buf()) {
        Ok(db) => db,
        Err(e) => return message_and_continue(&format!("Failed to open database: {}", e)),
    };

    // Copy the file
    if let Err(e) = add_to_destination(config, &db, path, &item.source) {
        return message_and_continue(&format!(
            "Failed to add '{}' to {} / {}:\n\n{}",
            display, item.source.repo_name, item.source.subdir_name, e
        ));
    }

    // Check how many sources exist now
    let config = Config::load(None)?;
    let sources = find_all_sources(&config, path)?;

    if sources.len() <= 1 {
        // Only one source - just tracking, no override needed
        return message_and_done(&format!(
            "Added '{}' to {} / {}\n\n\
            Note: This file is now tracked, but has no alternatives.\n\
            An override is only needed when multiple sources exist.",
            display, item.source.repo_name, item.source.subdir_name
        ));
    }

    // Multiple sources - set override
    let mut overrides = match OverrideConfig::load() {
        Ok(o) => o,
        Err(e) => {
            return message_and_done(&format!(
                "File was copied but failed to load overrides: {}\n\n\
                Use 'ins dot alternative {}' to switch sources.",
                e, display
            ));
        }
    };

    if let Err(e) = overrides.set_override(
        path.to_path_buf(),
        item.source.repo_name.clone(),
        item.source.subdir_name.clone(),
    ) {
        return message_and_done(&format!(
            "File was copied but failed to set override: {}\n\n\
            Use 'ins dot alternative {}' to switch sources.",
            e, display
        ));
    }

    message_and_done(&format!(
        "Created alternative for '{}' at {} / {}\n\n\
        This location is now set as the active source.\n\
        {} source(s) available.",
        display,
        item.source.repo_name,
        item.source.subdir_name,
        sources.len()
    ))
}

fn create_new_subdir(config: &Config, repo_name: &str) -> Result<bool> {
    use crate::dot::localrepo::LocalRepo;

    let new_dir = match FzfWrapper::builder()
        .prompt("New dotfile directory name: ")
        .args(fzf_mocha_args())
        .input()
        .input_result()?
    {
        FzfResult::Selected(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Ok(false),
    };

    let local_repo = LocalRepo::new(config, repo_name.to_string())?;
    let local_path = local_repo.local_path(config)?;

    match crate::dot::meta::add_dots_dir(&local_path, &new_dir) {
        Ok(()) => {
            // Add to global config
            let mut config = Config::load(None)?;
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name) {
                let active_subdirs = repo.active_subdirectories.get_or_insert_with(Vec::new);
                if !active_subdirs.contains(&new_dir) {
                    active_subdirs.push(new_dir.clone());
                    config.save(None)?;
                }
            }

            emit(
                Level::Success,
                "dot.alternative.subdir_created",
                &format!(
                    "{} Created dotfile directory '{}/{}' - now select it",
                    char::from(NerdFont::Check),
                    repo_name.green(),
                    new_dir.green()
                ),
                None,
            );
            Ok(true)
        }
        Err(e) => {
            FzfWrapper::message(&format!("Failed to create directory: {}", e))?;
            Ok(false)
        }
    }
}

fn clone_new_repo() -> Result<bool> {
    let mut config = Config::load(None)?;
    let db = Database::new(config.database_path().to_path_buf())?;
    let original_count = config.repos.len();

    crate::dot::menu::add_repo::handle_add_repo(&mut config, &db, false)?;

    Ok(config.repos.len() > original_count)
}
