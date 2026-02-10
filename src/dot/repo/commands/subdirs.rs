use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::repo::DotfileRepositoryManager;
use crate::dot::repo::cli::SubdirCommands;
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;
use colored::*;

use super::apply::apply_all_repos;

/// Handle subdirectory commands
pub(super) fn handle_subdir_command(
    config: &mut DotfileConfig,
    db: &Database,
    command: &SubdirCommands,
) -> Result<()> {
    match command {
        SubdirCommands::List { name, active } => list_subdirectories(config, db, name, *active),
        SubdirCommands::Set { name, subdirs } => set_subdirectories(config, name, subdirs),
        SubdirCommands::Enable { name, subdir } => enable_subdirectory(config, db, name, subdir),
        SubdirCommands::Disable { name, subdir } => disable_subdirectory(config, name, subdir),
    }
}

/// List subdirectories for a repository
fn list_subdirectories(
    config: &DotfileConfig,
    db: &Database,
    name: &str,
    active_only: bool,
) -> Result<()> {
    let repo_manager = DotfileRepositoryManager::new(config, db);

    let dotfile_repo = repo_manager.get_repository_info(name)?;
    let repo_config = config
        .repos
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", name))?;

    println!("Subdirectories for repository '{}':", name.cyan());

    for dir in &dotfile_repo.dotfile_dirs {
        let dir_name = dir
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        if active_only && !dir.is_active {
            continue;
        }

        let status = if dir.is_active {
            "active".green()
        } else {
            "inactive".yellow()
        };
        let configured = repo_config
            .active_subdirectories
            .as_ref()
            .map(|subdirs| subdirs.contains(&dir_name.to_string()))
            .unwrap_or(false);
        let configured_status = if configured {
            "configured".blue()
        } else {
            "not configured".dimmed()
        };

        println!("  {dir_name} - {status} ({configured_status})");
    }

    Ok(())
}

/// Set active subdirectories for a repository
fn set_subdirectories(config: &mut DotfileConfig, name: &str, subdirs: &[String]) -> Result<()> {
    config.set_active_subdirs(name, subdirs.to_vec(), None)?;
    println!(
        "{} active subdirectories for repository '{}': {}",
        "Set".green(),
        name,
        subdirs.join(", ")
    );
    Ok(())
}

/// Enable a subdirectory for a repository
fn enable_subdirectory(
    config: &mut DotfileConfig,
    db: &Database,
    name: &str,
    subdir: &str,
) -> Result<()> {
    // First verify the subdir exists in the repo's metadata
    let dotfile_repo = crate::dot::dotfilerepo::DotfileRepo::new(config, name.to_string())?;
    if !dotfile_repo.meta.dots_dirs.contains(&subdir.to_string()) {
        return Err(anyhow::anyhow!(
            "Subdirectory '{}' not found in repository '{}'. Available: {}",
            subdir,
            name,
            dotfile_repo.meta.dots_dirs.join(", ")
        ));
    }

    // Get current active subdirs
    let mut active = config.get_active_subdirs(name);

    if active.contains(&subdir.to_string()) {
        println!(
            "{} Subdirectory '{}' is already enabled for '{}'",
            char::from(NerdFont::Info),
            subdir,
            name
        );
        return Ok(());
    }

    active.push(subdir.to_string());
    config.set_active_subdirs(name, active, None)?;

    println!(
        "{} Enabled subdirectory '{}' for repository '{}'",
        char::from(NerdFont::Check).to_string().green(),
        subdir,
        name
    );

    // Apply to pick up new dotfiles
    apply_all_repos(config, db)?;

    Ok(())
}

/// Disable a subdirectory for a repository
fn disable_subdirectory(config: &mut DotfileConfig, name: &str, subdir: &str) -> Result<()> {
    let mut active = config.get_active_subdirs(name);

    if !active.contains(&subdir.to_string()) {
        println!(
            "{} Subdirectory '{}' is not enabled for '{}'",
            char::from(NerdFont::Info),
            subdir,
            name
        );
        return Ok(());
    }

    // Check if this is the last active subdir
    if active.len() == 1 {
        return Err(anyhow::anyhow!(
            "Cannot disable the last active subdirectory '{}'. At least one subdirectory must remain active.",
            subdir
        ));
    }

    active.retain(|s| s != subdir);

    config.set_active_subdirs(name, active, None)?;

    println!(
        "{} Disabled subdirectory '{}' for repository '{}'",
        char::from(NerdFont::Check).to_string().green(),
        subdir,
        name
    );

    Ok(())
}
