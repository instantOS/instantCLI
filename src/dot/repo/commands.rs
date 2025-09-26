use super::cli::{RepoCommands, SubdirCommands};
use crate::dot::config::{ConfigManager, extract_repo_name};
use crate::dot::db::Database;
use crate::dot::git::add_repo as git_add_repo;
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

    // Clone the repository
    let target_path = git_add_repo(config_manager, repo_config, debug)?;

    println!(
        "{} repository '{}' from {}",
        "Added".green(),
        repo_name,
        url
    );

    println!("Repository cloned to: {}", target_path.display());

    Ok(())
}
/// Remove a repository
fn remove_repository(
    config_manager: &mut ConfigManager,
    _db: &Database,
    name: &str,
    remove_files: bool,
) -> Result<()> {
    let config = config_manager.config();
    
    // Check if repository exists in config
    if !config.repos.iter().any(|r| r.name == name) {
        return Err(anyhow::anyhow!("Repository '{}' not found", name));
    }

    if remove_files {
        // Remove repository files
        let repo_path = config.repos_path().join(name);
        if repo_path.exists() {
            std::fs::remove_dir_all(&repo_path).with_context(|| {
                format!("Failed to remove repository directory: {}", repo_path.display())
            })?;
            println!("Removed repository files from: {}", repo_path.display());
        }
    }

    // Remove from configuration
    config_manager.remove_repo(name)?;

    println!("{} repository '{}'", "Removed".green(), name);

    Ok(())
}

/// Show detailed repository information
fn show_repository_info(config_manager: &ConfigManager, _db: &Database, name: &str) -> Result<()> {
    let config = config_manager.config();
    
    let repo = config.repos.iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", name))?;

    let local_path = config.repos_path().join(name);
    let exists = local_path.exists();

    println!("Repository: {}", name.cyan());
    println!("URL: {}", repo.url);
    println!("Branch: {}", repo.branch.as_deref().unwrap_or("default"));
    println!(
        "Status: {}",
        if repo.enabled {
            "enabled".green()
        } else {
            "disabled".yellow()
        }
    );
    println!("Local path: {}", local_path.display());
    println!("Exists: {}", if exists { "yes".green() } else { "no".yellow() });

    if exists {
        // Try to get repository metadata
        if let Ok(local_repo) = crate::dot::localrepo::LocalRepo::new(config, name.to_string()) {
            println!("Description: {}", local_repo.meta.description.as_deref().unwrap_or("No description"));
            println!("Available subdirectories: {}", local_repo.meta.dots_dirs.join(", "));
        }
    }

    println!("Active subdirectories: {}", repo.active_subdirectories.join(", "));

    Ok(())
}

/// Enable a repository
fn enable_repository(config_manager: &mut ConfigManager, name: &str) -> Result<()> {
    config_manager.with_config_mut(|config| {
        if let Some(repo) = config.repos.iter_mut().find(|r| r.name == name) {
            repo.enabled = true;
            println!("{} repository '{}'", "Enabled".green(), name);
        } else {
            return Err(anyhow::anyhow!("Repository '{}' not found", name));
        }
        Ok(())
    })?;
    Ok(())
}

/// Disable a repository
fn disable_repository(config_manager: &mut ConfigManager, name: &str) -> Result<()> {
    config_manager.with_config_mut(|config| {
        if let Some(repo) = config.repos.iter_mut().find(|r| r.name == name) {
            repo.enabled = false;
            println!("{} repository '{}'", "Disabled".yellow(), name);
        } else {
            return Err(anyhow::anyhow!("Repository '{}' not found", name));
        }
        Ok(())
    })?;
    Ok(())
}

/// Handle subdirectory commands
fn handle_subdir_command(
    config_manager: &mut ConfigManager,
    _db: &Database,
    command: &SubdirCommands,
) -> Result<()> {
    match command {
        SubdirCommands::List { name, active } => {
            list_subdirectories(config_manager, name, *active)
        }
        SubdirCommands::Set { name, subdirs } => set_subdirectories(config_manager, name, subdirs),
    }
}

/// List subdirectories for a repository
fn list_subdirectories(
    config_manager: &ConfigManager,
    name: &str,
    active_only: bool,
) -> Result<()> {
    let config = config_manager.config();
    
    let repo = config.repos.iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", name))?;

    println!("Subdirectories for repository '{}':", name.cyan());

    // Try to get available subdirectories from repository metadata
    let available_subdirs = if let Ok(local_repo) = crate::dot::localrepo::LocalRepo::new(config, name.to_string()) {
        local_repo.meta.dots_dirs
    } else {
        vec!["dots".to_string()] // default fallback
    };

    for dir_name in &available_subdirs {
        let is_active = repo.active_subdirectories.contains(dir_name);

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
    config_manager: &mut ConfigManager,
    name: &str,
    subdirs: &[String],
) -> Result<()> {
    config_manager.with_config_mut(|config| {
        if let Some(repo) = config.repos.iter_mut().find(|r| r.name == name) {
            repo.active_subdirectories = subdirs.to_vec();
            println!(
                "{} active subdirectories for repository '{}': {}",
                "Set".green(),
                name,
                subdirs.join(", ")
            );
        } else {
            return Err(anyhow::anyhow!("Repository '{}' not found", name));
        }
        Ok(())
    })?;
    Ok(())
}

