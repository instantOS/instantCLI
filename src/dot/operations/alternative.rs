//! Alternative source selection for dotfiles
//!
//! Allows users to interactively select which repository/subdirectory
//! a dotfile should be sourced from.

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::dot::config::Config;
use crate::dot::override_config::{DotfileSource, OverrideConfig, find_all_sources};
use crate::dot::utils::resolve_dotfile_path;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

/// Wrapper for DotfileSource to implement FzfSelectable
#[derive(Clone)]
struct SourceSelectItem {
    source: DotfileSource,
    is_current: bool,
    exists: bool,
}

impl FzfSelectable for SourceSelectItem {
    fn fzf_display_text(&self) -> String {
        let current = if self.is_current { " (current)" } else { "" };
        let status = if self.exists { "" } else { " [new]" };
        format!(
            "{} / {}{}{}",
            self.source.repo_name, self.source.subdir_name, current, status
        )
    }

    fn fzf_key(&self) -> String {
        format!("{}:{}", self.source.repo_name, self.source.subdir_name)
    }
}

/// Handle the alternative command
pub fn handle_alternative(config: &Config, path: &str, reset: bool, create: bool) -> Result<()> {
    let target_path = resolve_dotfile_path(path)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let display_path = target_path
        .strip_prefix(&home)
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| target_path.display().to_string());

    // Handle reset flag
    if reset {
        return handle_reset(&target_path, &display_path);
    }

    // Find all sources for this file
    let sources = find_all_sources(config, &target_path)?;

    if create {
        return handle_create(config, &target_path, &display_path, &sources);
    }

    if sources.is_empty() {
        emit(
            Level::Warn,
            "dot.alternative.not_found",
            &format!(
                "{} No dotfile sources found for {}. Use --create to add it to a repo.",
                char::from(NerdFont::Warning),
                display_path.yellow()
            ),
            None,
        );
        return Ok(());
    }

    if sources.len() == 1 {
        let source = &sources[0];
        emit(
            Level::Info,
            "dot.alternative.single_source",
            &format!(
                "{} {} is only available from {} / {}",
                char::from(NerdFont::Info),
                display_path.cyan(),
                source.repo_name.green(),
                source.subdir_name.green()
            ),
            None,
        );
        return Ok(());
    }

    // Load existing overrides to mark current selection
    let overrides = OverrideConfig::load()?;
    let current_override = overrides.get_override(&target_path);

    // Build selection items
    let items: Vec<SourceSelectItem> = sources
        .into_iter()
        .map(|source| {
            let is_current = current_override
                .map(|o| o.source_repo == source.repo_name && o.source_subdir == source.subdir_name)
                .unwrap_or(false);
            SourceSelectItem { source, is_current, exists: true }
        })
        .collect();

    // Show picker
    let prompt = format!("Select source for {}: ", display_path);
    match FzfWrapper::builder().prompt(prompt).select(items)? {
        FzfResult::Selected(item) => {
            let mut overrides = OverrideConfig::load()?;
            overrides.set_override(
                target_path.clone(),
                item.source.repo_name.clone(),
                item.source.subdir_name.clone(),
            )?;

            emit(
                Level::Success,
                "dot.alternative.set",
                &format!(
                    "{} {} will now be sourced from {} / {}",
                    char::from(NerdFont::Check),
                    display_path.cyan(),
                    item.source.repo_name.green(),
                    item.source.subdir_name.green()
                ),
                Some(serde_json::json!({
                    "target": display_path,
                    "repo": item.source.repo_name,
                    "subdir": item.source.subdir_name
                })),
            );
        }
        FzfResult::Cancelled => {
            emit(
                Level::Info,
                "dot.alternative.cancelled",
                &format!("{} Selection cancelled", char::from(NerdFont::Info)),
                None,
            );
        }
        FzfResult::Error(e) => {
            return Err(anyhow::anyhow!("Selection error: {}", e));
        }
        _ => {}
    }

    Ok(())
}

/// Handle --create flag: add file to a chosen repo/subdir
fn handle_create(
    config: &Config,
    target_path: &PathBuf,
    display_path: &str,
    existing_sources: &[DotfileSource],
) -> Result<()> {
    use crate::dot::db::Database;
    
    // Get all available repos/subdirs
    let all_destinations = get_all_destinations(config)?;

    if all_destinations.is_empty() {
        emit(
            Level::Warn,
            "dot.alternative.no_repos",
            &format!(
                "{} No writable repositories configured",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(());
    }

    // Build selection items, marking which ones already have the file
    let items: Vec<SourceSelectItem> = all_destinations
        .into_iter()
        .map(|dest| {
            let exists = existing_sources.iter().any(|s| {
                s.repo_name == dest.repo_name && s.subdir_name == dest.subdir_name
            });
            SourceSelectItem {
                source: dest,
                is_current: false,
                exists,
            }
        })
        .collect();

    let prompt = format!("Select destination for {}: ", display_path);
    match FzfWrapper::builder().prompt(prompt).select(items)? {
        FzfResult::Selected(item) => {
            if item.exists {
                // Just set the override
                let mut overrides = OverrideConfig::load()?;
                overrides.set_override(
                    target_path.clone(),
                    item.source.repo_name.clone(),
                    item.source.subdir_name.clone(),
                )?;

                emit(
                    Level::Success,
                    "dot.alternative.set",
                    &format!(
                        "{} {} will now be sourced from {} / {}",
                        char::from(NerdFont::Check),
                        display_path.cyan(),
                        item.source.repo_name.green(),
                        item.source.subdir_name.green()
                    ),
                    None,
                );
            } else {
                // Use add_to_destination which handles file copying and DB registration
                let db = Database::new(config.database_path().to_path_buf())?;
                add_to_destination(config, &db, target_path, &item.source)?;

                // Set override to use this new source
                let mut overrides = OverrideConfig::load()?;
                overrides.set_override(
                    target_path.clone(),
                    item.source.repo_name.clone(),
                    item.source.subdir_name.clone(),
                )?;
            }
        }
        FzfResult::Cancelled => {
            emit(
                Level::Info,
                "dot.alternative.cancelled",
                &format!("{} Selection cancelled", char::from(NerdFont::Info)),
                None,
            );
        }
        FzfResult::Error(e) => {
            return Err(anyhow::anyhow!("Selection error: {}", e));
        }
        _ => {}
    }

    Ok(())
}

/// Get all available repo/subdir destinations (exported for use by add command)
pub fn get_all_destinations(config: &Config) -> Result<Vec<DotfileSource>> {
    let mut destinations = Vec::new();

    for repo_config in &config.repos {
        if !repo_config.enabled || repo_config.read_only {
            continue;
        }

        for subdir in &repo_config.active_subdirectories {
            destinations.push(DotfileSource {
                repo_name: repo_config.name.clone(),
                subdir_name: subdir.clone(),
                source_path: config.repos_path().join(&repo_config.name).join(subdir),
            });
        }
    }

    Ok(destinations)
}

/// Add a file to a specific destination (shared by both alternative and add commands)
pub fn add_to_destination(
    config: &Config,
    db: &crate::dot::db::Database,
    target_path: &PathBuf,
    dest: &DotfileSource,
) -> Result<()> {
    use crate::dot::dotfile::Dotfile;
    use std::fs;

    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative = target_path.strip_prefix(&home).unwrap_or(target_path);
    let dest_path = dest.source_path.join(relative);

    // Ensure parent directory exists
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Use Dotfile to copy and register in DB
    let dotfile = Dotfile {
        source_path: dest_path.clone(),
        target_path: target_path.clone(),
    };
    dotfile.create_source_from_target(db)?;

    // Automatically stage the new file
    let repo_path = config.repos_path().join(&dest.repo_name);
    if let Err(e) = crate::dot::git::repo_ops::git_add(&repo_path, &dest_path, false) {
        eprintln!(
            "{} Failed to stage file: {}",
            char::from(NerdFont::Warning).to_string().yellow(),
            e
        );
    }

    let relative_display = relative.display().to_string();
    emit(
        Level::Success,
        "dot.add.created",
        &format!(
            "{} Added ~/{} to {} / {}",
            char::from(NerdFont::Check),
            relative_display.green(),
            dest.repo_name.green(),
            dest.subdir_name.green()
        ),
        None,
    );

    Ok(())
}

/// Handle the --reset flag
fn handle_reset(target_path: &PathBuf, display_path: &str) -> Result<()> {
    let mut overrides = OverrideConfig::load()?;

    if overrides.remove_override(target_path)? {
        emit(
            Level::Success,
            "dot.alternative.reset",
            &format!(
                "{} Removed override for {} (now using default priority)",
                char::from(NerdFont::Check),
                display_path.cyan()
            ),
            Some(serde_json::json!({
                "target": display_path,
                "action": "reset"
            })),
        );
    } else {
        emit(
            Level::Info,
            "dot.alternative.no_override",
            &format!(
                "{} No override exists for {}",
                char::from(NerdFont::Info),
                display_path.cyan()
            ),
            None,
        );
    }

    Ok(())
}
