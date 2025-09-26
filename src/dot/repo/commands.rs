use super::cli::{RepoCommands, SubdirCommands};
use crate::dot::config::extract_repo_name;
use crate::dot::repository::RepositoryService;
use anyhow::{Context, Result};
use colored::*;

/// Handle repository subcommands
pub fn handle_repo_command(
    repository_service: RepositoryService,
    command: &RepoCommands,
    debug: bool,
) -> Result<()> {
    match command {
        RepoCommands::List => list_repositories(&repository_service),
        RepoCommands::Add { url, name, branch } => add_repository(
            repository_service,
            url,
            name.as_deref(),
            branch.as_deref(),
            debug,
        ),
        RepoCommands::Remove { name, keep_files } => {
            remove_repository(repository_service, name, !*keep_files)
        }
        RepoCommands::Info { name } => show_repository_info(&repository_service, name),
        RepoCommands::Enable { name } => enable_repository(repository_service, name),
        RepoCommands::Disable { name } => disable_repository(repository_service, name),
        RepoCommands::Subdirs { command } => handle_subdir_command(repository_service, command),
    }
}

/// List all configured repositories
fn list_repositories(repository_service: &RepositoryService) -> Result<()> {
    let repositories = repository_service.list_repositories();

    if repositories.is_empty() {
        println!("No repositories configured.");
        return Ok(());
    }

    println!("Configured repositories:");
    for repo_status in repositories {
        let status = if repo_status.enabled {
            "enabled".green()
        } else {
            "disabled".yellow()
        };

        let branch_info = repo_status
            .branch
            .as_deref()
            .map(|b| format!(" ({b})"))
            .unwrap_or_default();

        let active_subdirs = if repo_status.active_subdirs.is_empty() {
            "dots".to_string()
        } else {
            repo_status.active_subdirs.join(", ")
        };

        // Get the URL from the underlying repo config
        let repo = repository_service.get_repository(&repo_status.name);
        let url = repo.map(|r| r.url.clone()).unwrap_or_else(|_| "unknown".to_string());

        println!(
            "  {}{} - {} [{}]",
            repo_status.name.cyan(),
            branch_info,
            url,
            status
        );
        println!("    Active subdirs: {active_subdirs}");
    }

    Ok(())
}

/// Add a new repository
fn add_repository(
    mut repository_service: RepositoryService,
    url: &str,
    name: Option<&str>,
    branch: Option<&str>,
    debug: bool,
) -> Result<()> {
    let repo_name = name
        .map(|s| s.to_string())
        .unwrap_or_else(|| extract_repo_name(url));

    // Add and clone the repository
    repository_service.add_repository(url, name, branch)?;

    println!(
        "{} repository '{}' from {}",
        "Added".green(),
        repo_name,
        url
    );

    // TODO: Apply dotfiles from new repository after adding
    // This would need to be handled by the RepositoryService

    Ok(())
}

/// Remove a repository
fn remove_repository(
    mut repository_service: RepositoryService,
    name: &str,
    remove_files: bool,
) -> Result<()> {
    if remove_files {
        // Get repository info before removal to check if it exists
        if let Ok(repo_info) = repository_service.get_repository_info(name) {
            if repo_info.exists {
                if repo_info.local_path.exists() {
                    std::fs::remove_dir_all(&repo_info.local_path).with_context(|| {
                        format!(
                            "Failed to remove repository directory: {}",
                            repo_info.local_path.display()
                        )
                    })?;
                    println!("Removed repository files from: {}", repo_info.local_path.display());
                }
            }
        }
    }

    // Remove from configuration
    repository_service.remove_repository(name)?;

    println!("{} repository '{}'", "Removed".green(), name);

    Ok(())
}

/// Show detailed repository information
fn show_repository_info(repository_service: &RepositoryService, name: &str) -> Result<()> {
    let repo_info = repository_service.get_repository_info(name)?;

    println!("Repository: {}", name.cyan());
    println!("URL: {}", repo_info.url);
    println!(
        "Branch: {}",
        repo_info.branch.as_deref().unwrap_or("default")
    );
    println!(
        "Status: {}",
        if repo_info.enabled {
            "enabled".green()
        } else {
            "disabled".yellow()
        }
    );
    println!("Local path: {}", repo_info.local_path.display());
    println!("Exists: {}", if repo_info.exists { "yes".green() } else { "no".yellow() });

    if let Some(description) = &repo_info.description {
        println!("Description: {description}");
    }
    if let Some(last_updated) = &repo_info.last_updated {
        println!("Last updated: {last_updated}");
    }

    println!("\nActive subdirectories: {}", repo_info.active_subdirs.join(", "));
    println!("Available subdirectories: {}", repo_info.available_subdirs.join(", "));

    Ok(())
}

/// Enable a repository
fn enable_repository(mut repository_service: RepositoryService, name: &str) -> Result<()> {
    repository_service.enable_repository(name)?;
    println!("{} repository '{}'", "Enabled".green(), name);
    Ok(())
}

/// Disable a repository
fn disable_repository(mut repository_service: RepositoryService, name: &str) -> Result<()> {
    repository_service.disable_repository(name)?;
    println!("{} repository '{}'", "Disabled".yellow(), name);
    Ok(())
}

/// Handle subdirectory commands
fn handle_subdir_command(
    mut repository_service: RepositoryService,
    command: &SubdirCommands,
) -> Result<()> {
    match command {
        SubdirCommands::List { name, active } => {
            list_subdirectories(&repository_service, name, *active)
        }
        SubdirCommands::Set { name, subdirs } => set_subdirectories(repository_service, name, subdirs),
    }
}

/// List subdirectories for a repository
fn list_subdirectories(
    repository_service: &RepositoryService,
    name: &str,
    active_only: bool,
) -> Result<()> {
    let repo_info = repository_service.get_repository_info(name)?;

    println!("Subdirectories for repository '{}':", name.cyan());

    for dir_name in &repo_info.available_subdirs {
        let is_active = repo_info.active_subdirs.contains(dir_name);

        if active_only && !is_active {
            continue;
        }

        let status = if is_active {
            "active".green()
        } else {
            "inactive".yellow()
        };

        println!("  {dir_name} - {status}");
    }

    Ok(())
}

/// Set active subdirectories for a repository
fn set_subdirectories(
    mut repository_service: RepositoryService,
    name: &str,
    subdirs: &[String],
) -> Result<()> {
    repository_service.set_active_subdirectories(name, subdirs.to_vec())?;
    println!(
        "{} active subdirectories for repository '{}': {}",
        "Set".green(),
        name,
        subdirs.join(", ")
    );
    Ok(())
}

