use super::cli::{RepoCommands, SubdirCommands};
use crate::dot::config::{ConfigManager, extract_repo_name};
use crate::dot::db::Database;
use crate::dot::git::add_repo as git_add_repo;
use crate::dot::repo::RepositoryManager;
use crate::ui::Level;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::*;

/// Handle repository subcommands
pub fn handle_repo_command(
    config_manager: &mut ConfigManager,
    db: &Database,
    command: &RepoCommands,
    debug: bool,
) -> Result<()> {
    match command {
        RepoCommands::List => list_repositories(config_manager, db),
        RepoCommands::Add { url, name, branch } => add_repository(
            config_manager,
            db,
            url,
            name.as_deref(),
            branch.as_deref(),
            debug,
        ),
        RepoCommands::Remove { name, keep_files } => {
            remove_repository(config_manager, db, name, !*keep_files)
        }
        RepoCommands::Info { name } => show_repository_info(config_manager, db, name),
        RepoCommands::Enable { name } => enable_repository(config_manager, name),
        RepoCommands::Disable { name } => disable_repository(config_manager, name),
        RepoCommands::Subdirs { command } => handle_subdir_command(config_manager, db, command),
    }
}

/// List all configured repositories
fn list_repositories(config_manager: &ConfigManager, _db: &Database) -> Result<()> {
    let config = config_manager.config();

    if config.repos.is_empty() {
        println!("No repositories configured.");
        return Ok(());
    }

    println!("Configured repositories:");
    for repo_config in &config.repos {
        let status = if repo_config.enabled {
            "enabled".green()
        } else {
            "disabled".yellow()
        };

        let branch_info = repo_config
            .branch
            .as_deref()
            .map(|b| format!(" ({b})"))
            .unwrap_or_default();

        let active_subdirs = if repo_config.active_subdirectories.is_empty() {
            "dots".to_string()
        } else {
            repo_config.active_subdirectories.join(", ")
        };

        println!(
            "  {}{} - {} [{}]",
            repo_config.name.cyan(),
            branch_info,
            repo_config.url,
            status
        );
        println!("    Active subdirs: {active_subdirs}");
    }

    Ok(())
}

/// Add a new repository
fn add_repository(
    config_manager: &mut ConfigManager,
    db: &Database,
    url: &str,
    name: Option<&str>,
    branch: Option<&str>,
    debug: bool,
) -> Result<()> {
    let repo_name = name
        .map(|s| s.to_string())
        .unwrap_or_else(|| extract_repo_name(url));

    // Create the repo config
    let repo_config = crate::dot::config::Repo {
        url: url.to_string(),
        name: repo_name.clone(),
        branch: branch.map(|s| s.to_string()),
        active_subdirectories: vec!["dots".to_string()],
        enabled: true,
    };

    // Add the repo to config
    config_manager.add_repo(repo_config.clone())?;

    emit(
        Level::Success,
        "dot.repo.added",
        &format!(
            "{} Added repository '{}' from {}",
            char::from(Fa::Check),
            repo_name,
            url
        ),
        None,
    );

    // Clone the repository
    match git_add_repo(config_manager, repo_config, debug) {
        Ok(path) => {
            emit(
                Level::Info,
                "dot.repo.add.clone_path",
                &format!("{} Cloned to: {}", char::from(Fa::Folder), path.display()),
                None,
            );

            // Apply the repository immediately after adding
            emit(
                Level::Info,
                "dot.repo.add.apply",
                &format!(
                    "{} Applying dotfiles from new repository...",
                    char::from(Fa::InfoCircle)
                ),
                None,
            );
            if let Err(e) = apply_all_repos(config_manager, db) {
                emit(
                    Level::Warn,
                    "dot.repo.add.apply_failed",
                    &format!(
                        "{} Failed to apply dotfiles: {e}",
                        char::from(Fa::ExclamationCircle)
                    ),
                    None,
                );
            }
        }
        Err(e) => {
            emit(
                Level::Error,
                "dot.repo.add.clone_failed",
                &format!(
                    "{} Failed to clone repository: {e}",
                    char::from(Fa::TimesCircle)
                ),
                None,
            );
            // Remove from config since clone failed
            config_manager.remove_repo(&repo_name)?;
            return Err(e);
        }
    }

    Ok(())
}

/// Remove a repository
fn remove_repository(
    config_manager: &mut ConfigManager,
    db: &Database,
    name: &str,
    remove_files: bool,
) -> Result<()> {
    let config = config_manager.config();

    // Find the repository
    let _repo_index = config
        .repos
        .iter()
        .position(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", name))?;

    if remove_files {
        // Remove the local files
        let repo_manager = RepositoryManager::new(config, db);
        if let Ok(local_repo) = repo_manager.get_repository_info(name) {
            let local_path = local_repo.local_path(config)?;
            if local_path.exists() {
                std::fs::remove_dir_all(&local_path).with_context(|| {
                    format!(
                        "Failed to remove repository directory: {}",
                        local_path.display()
                    )
                })?;
                println!("Removed repository files from: {}", local_path.display());
            }
        }
    }

    // Remove from config
    config_manager.remove_repo(name)?;

    println!("{} repository '{}'", "Removed".green(), name);

    Ok(())
}

/// Show detailed repository information
fn show_repository_info(config_manager: &ConfigManager, db: &Database, name: &str) -> Result<()> {
    let config = config_manager.config();
    let repo_manager = RepositoryManager::new(config, db);

    let local_repo = repo_manager.get_repository_info(name)?;
    let repo_config = config
        .repos
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found in configuration", name))?;

    let local_path = local_repo.local_path(config)?.display().to_string();
    let status_text = if repo_config.enabled {
        "Enabled".green().to_string()
    } else {
        "Disabled".yellow().to_string()
    };

    let mut rows: Vec<(char, &str, String)> = vec![
        (
            char::from(Fa::Folder),
            "Repository",
            name.cyan().to_string(),
        ),
        (char::from(Fa::InfoCircle), "URL", repo_config.url.clone()),
        (
            char::from(Fa::Flag),
            "Branch",
            repo_config
                .branch
                .as_deref()
                .unwrap_or("default")
                .to_string(),
        ),
        (char::from(Fa::Check), "Status", status_text),
        (char::from(Fa::Folder), "Local Path", local_path),
    ];

    if let Some(author) = &local_repo.meta.author {
        rows.push((char::from(Fa::InfoCircle), "Author", author.clone()));
    }
    if let Some(description) = &local_repo.meta.description {
        rows.push((
            char::from(Fa::InfoCircle),
            "Description",
            description.clone(),
        ));
    }

    let label_width = rows
        .iter()
        .map(|(_, label, _)| label.len())
        .max()
        .unwrap_or(0)
        + 1;

    println!();
    println!(
        "{} {}",
        char::from(Fa::List),
        "Repository Information".bold()
    );

    for (icon, label, value) in rows {
        println!(
            "  {} {:<width$} {}",
            icon,
            format!("{}:", label),
            value,
            width = label_width + 1
        );
    }

    println!();
    println!("{} {}", char::from(Fa::List), "Subdirectories".bold());

    if local_repo.dotfile_dirs.is_empty() {
        println!(
            "  {} {}",
            char::from(Fa::InfoCircle),
            "No dotfile directories discovered.".dimmed()
        );
        return Ok(());
    }

    let dir_name_width = local_repo
        .dotfile_dirs
        .iter()
        .map(|dir| {
            dir.path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .len()
        })
        .max()
        .unwrap_or(0);

    for dir in &local_repo.dotfile_dirs {
        let dir_name = dir
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let status_icon = if dir.is_active {
            char::from(Fa::Check)
        } else {
            char::from(Fa::TimesCircle)
        };
        let status_text = if dir.is_active {
            "Active".green().to_string()
        } else {
            "Inactive".yellow().to_string()
        };
        let configured = repo_config.active_subdirectories.contains(&dir_name);
        let configured_label = if configured {
            "configured".blue().to_string()
        } else {
            "not configured".dimmed().to_string()
        };

        println!(
            "  {} {:<name_width$} {}  ({})  {}",
            status_icon,
            dir_name,
            status_text,
            configured_label,
            dir.path.display(),
            name_width = dir_name_width + 2
        );
    }

    Ok(())
}

/// Enable a repository
fn enable_repository(config_manager: &mut ConfigManager, name: &str) -> Result<()> {
    config_manager.enable_repo(name)?;
    println!("{} repository '{}'", "Enabled".green(), name);
    Ok(())
}

/// Disable a repository
fn disable_repository(config_manager: &mut ConfigManager, name: &str) -> Result<()> {
    config_manager.disable_repo(name)?;
    println!("{} repository '{}'", "Disabled".yellow(), name);
    Ok(())
}

/// Handle subdirectory commands
fn handle_subdir_command(
    config_manager: &mut ConfigManager,
    db: &Database,
    command: &SubdirCommands,
) -> Result<()> {
    match command {
        SubdirCommands::List { name, active } => {
            list_subdirectories(config_manager, db, name, *active)
        }
        SubdirCommands::Set { name, subdirs } => set_subdirectories(config_manager, name, subdirs),
    }
}

/// List subdirectories for a repository
fn list_subdirectories(
    config_manager: &ConfigManager,
    db: &Database,
    name: &str,
    active_only: bool,
) -> Result<()> {
    let config = config_manager.config();
    let repo_manager = RepositoryManager::new(config, db);

    let local_repo = repo_manager.get_repository_info(name)?;
    let repo_config = config
        .repos
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", name))?;

    println!("Subdirectories for repository '{}':", name.cyan());

    for dir in &local_repo.dotfile_dirs {
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
            .contains(&dir_name.to_string());
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
fn set_subdirectories(
    config_manager: &mut ConfigManager,
    name: &str,
    subdirs: &[String],
) -> Result<()> {
    config_manager.set_active_subdirs(name, subdirs.to_vec())?;
    println!(
        "{} active subdirectories for repository '{}': {}",
        "Set".green(),
        name,
        subdirs.join(", ")
    );
    Ok(())
}

/// Apply all repositories (helper function)
fn apply_all_repos(config_manager: &ConfigManager, db: &Database) -> Result<()> {
    use crate::dot::apply_all;

    apply_all(&config_manager.config, db)?;
    Ok(())
}
