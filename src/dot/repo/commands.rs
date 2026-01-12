use super::cli::{RepoCommands, SubdirCommands};
use crate::common::TildePath;
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
        RepoCommands::Clone(args) => clone_repository(
            config,
            db,
            &args.url,
            args.name.as_deref(),
            args.branch.as_deref(),
            args.read_only,
            args.force_write,
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
        RepoCommands::Status { name } => show_repository_status(config, db, name.as_deref()),
    }
}

/// Get local path in tilde notation for a repository
fn get_local_path_tilde(
    config: &Config,
    repo_manager: &RepositoryManager,
    repo_name: &str,
) -> String {
    repo_manager
        .get_repository_info(repo_name)
        .map(|local_repo| {
            let path = local_repo.local_path(config).unwrap_or_default();
            let tilde_path = TildePath::new(path.clone());
            tilde_path
                .to_tilde_string()
                .unwrap_or_else(|_| path.display().to_string())
        })
        .unwrap_or_else(|_| "Not found".to_string())
}

/// List all configured repositories
fn list_repositories(config: &Config, db: &Database) -> Result<()> {
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

    let repo_manager = RepositoryManager::new(config, db);

    match get_output_format() {
        OutputFormat::Json => {
            let repos_data: Vec<_> = config
                .repos
                .iter()
                .map(|repo_config| {
                    let local_path = get_local_path_tilde(config, &repo_manager, &repo_config.name);

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
                        "read_only": repo_config.read_only,
                        "local_path": local_path
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
            let total_repos = config.repos.len();

            for (index, repo_config) in config.repos.iter().enumerate() {
                // Priority: P1 = highest (first repo), P2 = second highest, etc.
                let priority = index + 1;
                let priority_label = format!("[P{}]", priority).bright_purple().bold();

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

                // Show subdir priority when multiple active subdirs
                let active_subdirs = if repo_config.active_subdirectories.is_empty() {
                    "dots".to_string()
                } else {
                    let subdirs = repo_config.active_subdirectories.join(", ");
                    if repo_config.active_subdirectories.len() > 1 {
                        // Show priority order for multiple subdirs
                        format!("{} (priority: first=highest)", subdirs)
                    } else {
                        subdirs
                    }
                };

                // Get local path in tilde notation
                let local_path = get_local_path_tilde(config, &repo_manager, &repo_config.name);

                // Overall priority hint
                let priority_hint = if priority == 1 && total_repos > 1 {
                    format!(" {}", "(highest priority)".dimmed())
                } else if priority == total_repos && total_repos > 1 {
                    format!(" {}", "(lowest priority)".dimmed())
                } else {
                    String::new()
                };

                println!(
                    "  {} {}{} - {} [{}]{}{}",
                    priority_label,
                    repo_config.name.cyan(),
                    branch_info,
                    repo_config.url,
                    status,
                    read_only,
                    priority_hint
                );
                println!("    Local path: {}", local_path.dimmed());
                println!("    Active subdirs: {active_subdirs}");
            }
        }
    }

    Ok(())
}

/// Resolve repository name from provided name, metadata, or URL
fn resolve_repo_name(url: &str, name: Option<&str>) -> String {
    name.map(|s| s.to_string())
        .or_else(|| {
            // For local paths, try to read name from instantdots.toml
            let path = std::path::Path::new(url);
            if path.exists() {
                let canonical = path.canonicalize().ok()?;
                crate::dot::meta::read_meta(&canonical)
                    .ok()
                    .map(|meta| meta.name)
            } else {
                None
            }
        })
        .unwrap_or_else(|| extract_repo_name(url))
}

/// Configure an external (yadm/stow) repository after cloning
fn configure_external_repo(config: &mut Config, repo_name: &str, read_only: bool) -> Result<()> {
    emit(
        Level::Info,
        "dot.repo.clone.external",
        &format!(
            "{} Detected external dotfile repository (Yadm/Stow compatible)",
            char::from(NerdFont::Info)
        ),
        None,
    );

    for repo in &mut config.repos {
        if repo.name == repo_name {
            repo.active_subdirectories = vec![".".to_string()];
            repo.metadata = Some(crate::dot::types::RepoMetaData {
                name: repo_name.to_string(),
                author: None,
                description: None,
                read_only: if read_only { Some(true) } else { None },
                dots_dirs: vec![".".to_string()],
            });
            break;
        }
    }
    config.save(None)
}

/// Check if repository metadata requests read-only mode and update config
fn handle_read_only_metadata(config: &mut Config, db: &Database, repo_name: &str) -> Result<()> {
    if let Ok(local_repo) =
        crate::dot::repo::RepositoryManager::new(config, db).get_repository_info(repo_name)
        && let Some(true) = local_repo.meta.read_only
    {
        emit(
            Level::Info,
            "dot.repo.clone.read_only",
            &format!(
                "{} Repository requested read-only mode. Marking as read-only.",
                char::from(NerdFont::Info)
            ),
            None,
        );
        for repo in &mut config.repos {
            if repo.name == repo_name {
                repo.read_only = true;
                break;
            }
        }
        config.save(None)?;
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

    let repo_name = resolve_repo_name(url, name);

    let repo_config = crate::dot::config::Repo {
        url: url.to_string(),
        name: repo_name.clone(),
        branch: branch.map(|s| s.to_string()),
        active_subdirectories: vec!["dots".to_string()],
        enabled: true,
        read_only: read_only_flag,
        metadata: None,
    };

    config.add_repo(repo_config.clone(), None)?;

    emit(
        Level::Success,
        "dot.repo.clone.added",
        &format!(
            "{} Cloning repository '{}' from {}",
            char::from(NerdFont::Check),
            repo_name,
            url
        ),
        None,
    );

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

            // Detect and configure external (yadm/stow) repos
            if !path.join("instantdots.toml").exists() {
                configure_external_repo(config, &repo_name, read_only_flag)?;
            }

            // Apply dotfiles
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

            // Handle read-only metadata request
            if !read_only_flag && !force_write_flag {
                handle_read_only_metadata(config, db, &repo_name)?;
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
        SubdirCommands::Enable { name, subdir } => enable_subdirectory(config, db, name, subdir),
        SubdirCommands::Disable { name, subdir } => disable_subdirectory(config, name, subdir),
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

/// Enable a subdirectory for a repository
fn enable_subdirectory(config: &mut Config, db: &Database, name: &str, subdir: &str) -> Result<()> {
    // First verify the subdir exists in the repo's metadata
    let local_repo = crate::dot::localrepo::LocalRepo::new(config, name.to_string())?;
    if !local_repo.meta.dots_dirs.contains(&subdir.to_string()) {
        return Err(anyhow::anyhow!(
            "Subdirectory '{}' not found in repository '{}'. Available: {}",
            subdir,
            name,
            local_repo.meta.dots_dirs.join(", ")
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
fn disable_subdirectory(config: &mut Config, name: &str, subdir: &str) -> Result<()> {
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

/// Show git repository status (working directory and branch sync state)
fn show_repository_status(config: &Config, db: &Database, name: Option<&str>) -> Result<()> {
    let repo_manager = RepositoryManager::new(config, db);

    // Determine which repos to show
    let repos_to_show: Vec<_> = if let Some(name) = name {
        vec![name.to_string()]
    } else {
        config.repos.iter().map(|r| r.name.clone()).collect()
    };

    for repo_name in repos_to_show {
        let local_repo = match repo_manager.get_repository_info(&repo_name) {
            Ok(repo) => repo,
            Err(e) => {
                eprintln!(
                    "{} {}: {}",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan(),
                    e
                );
                continue;
            }
        };

        let repo_path = local_repo.local_path(config)?;

        let git_repo = match git2::Repository::open(&repo_path) {
            Ok(repo) => repo,
            Err(e) => {
                eprintln!(
                    "{} {}: Failed to open git repository: {}",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan(),
                    e
                );
                continue;
            }
        };

        let repo_status = match crate::common::git::get_repo_status(&git_repo) {
            Ok(status) => status,
            Err(e) => {
                eprintln!(
                    "{} {}: Failed to get repo status: {}",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan(),
                    e
                );
                continue;
            }
        };

        let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
            Some(config) => config,
            None => {
                eprintln!(
                    "{} {}: Repository not found in configuration",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan()
                );
                continue;
            }
        };

        let tilde_path = TildePath::new(repo_path.to_path_buf());
        let local_path = tilde_path
            .to_tilde_string()
            .unwrap_or_else(|_| repo_path.display().to_string());

        println!();
        println!(
            "{} {}",
            char::from(NerdFont::FolderGit),
            repo_name.bold().cyan()
        );

        // Working directory status
        let (icon, status_text) = if repo_status.working_dir_clean {
            (char::from(NerdFont::CheckCircle), "Clean".green())
        } else {
            (
                char::from(NerdFont::Edit),
                format!(
                    "Dirty [{} modified, {} untracked]",
                    repo_status.file_counts.modified, repo_status.file_counts.untracked
                )
                .yellow(),
            )
        };

        println!("  Working Directory:  {} {}", icon, status_text);

        // Branch sync status
        let (icon, _status_text) = match &repo_status.branch_sync {
            crate::common::git::BranchSyncStatus::UpToDate => {
                (char::from(NerdFont::Check), "Up-to-date".green())
            }
            crate::common::git::BranchSyncStatus::Ahead { commits } => (
                char::from(NerdFont::CloudUpload),
                format!("Ahead {} commits", commits).blue(),
            ),
            crate::common::git::BranchSyncStatus::Behind { commits } => (
                char::from(NerdFont::CloudDownload),
                format!("Behind {} commits", commits).blue(),
            ),
            crate::common::git::BranchSyncStatus::Diverged { ahead, behind } => (
                char::from(NerdFont::GitMerge),
                format!("Diverged ({} ahead, {} behind)", ahead, behind).red(),
            ),
            crate::common::git::BranchSyncStatus::NoRemote => {
                (char::from(NerdFont::Warning), "No remote".yellow())
            }
        };

        println!(
            "  Branch Status:       {} ({})",
            icon,
            repo_status.branch.dimmed()
        );
        println!("  URL:                 {}", repo_config.url.dimmed());
        println!("  Local Path:          {}", local_path.dimmed());
    }

    println!();

    Ok(())
}
