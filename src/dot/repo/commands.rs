use super::cli::{RepoCommands, SubdirCommands};
use crate::dot::config::{Config, extract_repo_name};
use crate::dot::db::Database;
use crate::dot::git::add_repo as git_clone_repo;
use crate::dot::repo::RepositoryManager;
use crate::ui::Level;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::*;

/// Handle repository subcommands
pub fn handle_repo_command(
    config: &mut Config,
    db: &Database,
    command: &RepoCommands,
    debug: bool,
) -> Result<()> {
    match command {
        RepoCommands::List => list_repositories(config, db),
        RepoCommands::Clone {
            url,
            name,
            branch,
            read_only,
            force_write,
        } => clone_repository(
            config,
            db,
            url,
            name.as_deref(),
            branch.as_deref(),
            *read_only,
            *force_write,
            debug,
        ),
        RepoCommands::Remove { name, keep_files } => {
            remove_repository(config, db, name, !*keep_files)
        }
        RepoCommands::Info { name } => show_repository_info(config, db, name),
        RepoCommands::Enable { name } => enable_repository(config, name),
        RepoCommands::Disable { name } => disable_repository(config, name),
        RepoCommands::Subdirs { command } => handle_subdir_command(config, db, command),
        RepoCommands::SetReadOnly { name, read_only } => {
            set_read_only_status(config, name, *read_only)
        }
    }
}

/// List all configured repositories
fn list_repositories(config: &Config, _db: &Database) -> Result<()> {
    if config.repos.is_empty() {
        match get_output_format() {
            OutputFormat::Json => {
                let data = serde_json::json!({
                    "repos": [],
                    "count": 0
                });
                emit(
                    Level::Info,
                    "dot.repo.list",
                    "No repositories configured",
                    Some(data),
                );
            }
            OutputFormat::Text => {
                println!("No repositories configured.");
            }
        }
        return Ok(());
    }

    match get_output_format() {
        OutputFormat::Json => {
            let repos_data: Vec<_> = config
                .repos
                .iter()
                .map(|repo_config| {
                    serde_json::json!({
                        "name": repo_config.name,
                        "url": repo_config.url,
                        "branch": repo_config.branch,
                        "enabled": repo_config.enabled,
                        "active_subdirectories": if repo_config.active_subdirectories.is_empty() {
                            vec!["dots"]
                        } else {
                            repo_config.active_subdirectories.iter().map(|s| s.as_str()).collect::<Vec<_>>()
                        },
                        "read_only": repo_config.read_only
                    })
                })
                .collect();

            let data = serde_json::json!({
                "repos": repos_data,
                "count": config.repos.len()
            });

            emit(
                Level::Info,
                "dot.repo.list",
                &format!("Configured repositories: {}", config.repos.len()),
                Some(data),
            );
        }
        OutputFormat::Text => {
            println!("Configured repositories:");
            for repo_config in &config.repos {
                let status = if repo_config.enabled {
                    "enabled".green()
                } else {
                    "disabled".yellow()
                };

                let read_only = if repo_config.read_only {
                    " [read-only]".yellow()
                } else {
                    "".clear()
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
                    "  {}{} - {} [{}]{}",
                    repo_config.name.cyan(),
                    branch_info,
                    repo_config.url,
                    status,
                    read_only
                );
                println!("    Active subdirs: {active_subdirs}");
            }
        }
    }

    Ok(())
}

/// Clone a new repository
pub fn clone_repository(
    config: &mut Config,
    db: &Database,
    url: &str,
    name: Option<&str>,
    branch: Option<&str>,
    read_only_flag: bool,
    force_write_flag: bool,
    debug: bool,
) -> Result<()> {
    if read_only_flag && force_write_flag {
        return Err(anyhow::anyhow!(
            "Cannot use both --read-only and --force-write flags at the same time"
        ));
    }

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
        read_only: read_only_flag,
        metadata: None,
    };

    // Add the repo to config
    config.add_repo(repo_config.clone(), None)?;

    emit(
        Level::Success,
        "dot.repo.clone.added",
        &format!(
            "{} Cloned repository '{}' from {}",
            char::from(NerdFont::Check),
            repo_name,
            url
        ),
        None,
    );

    // Clone the repository
    match git_clone_repo(config, repo_config, debug) {
        Ok(path) => {
            emit(
                Level::Info,
                "dot.repo.clone.path",
                &format!(
                    "{} Cloned to: {}",
                    char::from(NerdFont::Folder),
                    path.display()
                ),
                None,
            );

            // Apply the repository immediately after adding
            emit(
                Level::Info,
                "dot.repo.clone.apply",
                &format!(
                    "{} Applying dotfiles from new repository...",
                    char::from(NerdFont::Info)
                ),
                None,
            );
            if let Err(e) = apply_all_repos(config, db) {
                emit(
                    Level::Warn,
                    "dot.repo.clone.apply_failed",
                    &format!(
                        "{} Failed to apply dotfiles: {e}",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
            }

            // Check metadata for read-only request
            if !read_only_flag && !force_write_flag {
                if let Ok(local_repo) = crate::dot::repo::RepositoryManager::new(config, db)
                    .get_repository_info(&repo_name)
                {
                    if let Some(true) = local_repo.meta.read_only {
                        emit(
                            Level::Info,
                            "dot.repo.clone.read_only",
                            &format!(
                                "{} Repository requested read-only mode. Marking as read-only.",
                                char::from(NerdFont::Info)
                            ),
                            None,
                        );
                        // Update config to set read_only to true
                        for repo in &mut config.repos {
                            if repo.name == repo_name {
                                repo.read_only = true;
                                break;
                            }
                        }
                        config.save(None)?;
                    }
                }
            }
        }
        Err(e) => {
            emit(
                Level::Error,
                "dot.repo.clone.failed",
                &format!(
                    "{} Failed to clone repository: {e}",
                    char::from(NerdFont::CrossCircle)
                ),
                None,
            );
            // Remove from config since clone failed
            config.remove_repo(&repo_name, None)?;
            return Err(e);
        }
    }

    Ok(())
}

/// Remove a repository
fn remove_repository(
    config: &mut Config,
    db: &Database,
    name: &str,
    remove_files: bool,
) -> Result<()> {
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
    config.remove_repo(name, None)?;

    println!("{} repository '{}'", "Removed".green(), name);

    Ok(())
}

/// Show detailed repository information
fn show_repository_info(config: &Config, db: &Database, name: &str) -> Result<()> {
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

    let read_only_text = if repo_config.read_only {
        "Yes".yellow().to_string()
    } else {
        "No".green().to_string()
    };

    let mut rows: Vec<(char, &str, String)> = vec![
        (
            char::from(NerdFont::FolderGit),
            "Repository",
            name.cyan().to_string(),
        ),
        (char::from(NerdFont::Link), "URL", repo_config.url.clone()),
        (
            char::from(NerdFont::GitBranch),
            "Branch",
            repo_config
                .branch
                .as_deref()
                .unwrap_or("default")
                .to_string(),
        ),
        (char::from(NerdFont::Check), "Status", status_text),
        (char::from(NerdFont::Lock), "Read-only", read_only_text),
        (char::from(NerdFont::Folder), "Local Path", local_path),
    ];

    if let Some(author) = &local_repo.meta.author {
        rows.push((char::from(NerdFont::User), "Author", author.clone()));
    }
    if let Some(description) = &local_repo.meta.description {
        rows.push((
            char::from(NerdFont::FileText),
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
        char::from(NerdFont::List),
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
    println!("{} {}", char::from(NerdFont::List), "Subdirectories".bold());

    if local_repo.dotfile_dirs.is_empty() {
        println!(
            "  {} {}",
            char::from(NerdFont::Info),
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
            char::from(NerdFont::Check)
        } else {
            char::from(NerdFont::CrossCircle)
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
fn enable_repository(config: &mut Config, name: &str) -> Result<()> {
    config.enable_repo(name, None)?;
    println!("{} repository '{}'", "Enabled".green(), name);
    Ok(())
}

/// Disable a repository
fn disable_repository(config: &mut Config, name: &str) -> Result<()> {
    config.disable_repo(name, None)?;
    println!("{} repository '{}'", "Disabled".yellow(), name);
    Ok(())
}

/// Handle subdirectory commands
fn handle_subdir_command(
    config: &mut Config,
    db: &Database,
    command: &SubdirCommands,
) -> Result<()> {
    match command {
        SubdirCommands::List { name, active } => list_subdirectories(config, db, name, *active),
        SubdirCommands::Set { name, subdirs } => set_subdirectories(config, name, subdirs),
    }
}

/// List subdirectories for a repository
fn list_subdirectories(
    config: &Config,
    db: &Database,
    name: &str,
    active_only: bool,
) -> Result<()> {
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
fn set_subdirectories(config: &mut Config, name: &str, subdirs: &[String]) -> Result<()> {
    config.set_active_subdirs(name, subdirs.to_vec(), None)?;
    println!(
        "{} active subdirectories for repository '{}': {}",
        "Set".green(),
        name,
        subdirs.join(", ")
    );
    Ok(())
}

/// Apply all repositories (helper function)
fn apply_all_repos(config: &Config, db: &Database) -> Result<()> {
    use crate::dot::apply_all;

    apply_all(config, db)?;
    Ok(())
}

/// Set read-only status for a repository
fn set_read_only_status(config: &mut Config, name: &str, read_only: bool) -> Result<()> {
    for repo in &mut config.repos {
        if repo.name == name {
            repo.read_only = read_only;
            config.save(None)?;
            println!(
                "{} read-only status for repository '{}' to {}",
                "Set".green(),
                name,
                read_only
            );
            return Ok(());
        }
    }
    Err(anyhow::anyhow!("Repository '{}' not found", name))
}
