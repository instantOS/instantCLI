use anyhow::Result;
use colored::*;
use shellexpand;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

pub mod config;
pub mod db;
pub mod dotfile;
pub mod git;
pub mod localrepo;
pub mod meta;
pub mod path_serde;
pub mod utils;

#[cfg(test)]
mod path_tests;

pub use crate::dot::dotfile::Dotfile;
pub use git::{add_repo, status_all, update_all};

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::{DotfileDir, LocalRepo};

/// Resolve a path argument to an absolute path in the home directory
///
/// This function handles path resolution similar to git:
/// - If path starts with '~', expand it to home directory
/// - If path is absolute, validate it's within home directory
/// - If path is relative, resolve it relative to current working directory,
///   then validate it's within home directory
///
/// Returns the resolved absolute path if valid, or an error if:
/// - The path doesn't exist
/// - The path is outside the home directory
pub fn resolve_dotfile_path(path: &str) -> Result<PathBuf> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    let resolved_path = if path.starts_with('~') {
        // Expand tilde to home directory
        PathBuf::from(shellexpand::tilde(path).into_owned())
    } else if Path::new(path).is_absolute() {
        // Use absolute path as-is
        PathBuf::from(path)
    } else {
        // For relative paths, resolve relative to current working directory
        let current_dir = std::env::current_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
        current_dir.join(path)
    };

    // Canonicalize the path to resolve any symlinks or relative components
    let canonical_path = resolved_path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Failed to resolve path '{}': {}", path, e))?;

    // Validate that the path is within the home directory
    if !canonical_path.starts_with(&home) {
        return Err(anyhow::anyhow!(
            "Path '{}' is outside the home directory. Only files in {} are allowed.",
            canonical_path.display(),
            home.display()
        ));
    }

    Ok(canonical_path)
}

/// Get active dotfile directories from a single repository
fn get_repo_active_dirs(config: &Config, repo: &config::Repo) -> Result<Vec<PathBuf>> {
    let local_repo = LocalRepo::new(config, repo.name.clone())?;
    Ok(local_repo
        .dotfile_dirs
        .iter()
        .filter(|dir| dir.is_active)
        .map(|dir| dir.path.clone())
        .collect())
}

/// Get all active dotfile directories from all repositories
pub fn get_active_dotfile_dirs(config: &Config) -> Result<Vec<PathBuf>> {
    let mut active_dirs = Vec::new();

    // Process repos in order of their configuration (relevance)
    for repo in &config.repos {
        match get_repo_active_dirs(config, repo) {
            Ok(mut dirs) => {
                active_dirs.append(&mut dirs);
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Warning: skipping repo '{}': {}", repo.name, e).yellow()
                );
            }
        }
    }

    Ok(active_dirs)
}

pub fn get_all_dotfiles(config: &Config) -> Result<HashMap<PathBuf, Dotfile>> {
    let mut filemap = HashMap::new();
    let active_dirs = get_active_dotfile_dirs(config)?;
    let home_path = PathBuf::from(shellexpand::tilde("~").to_string());

    // Process active dotfile directories in order of relevance
    for dir_path in active_dirs {
        for entry in WalkDir::new(&dir_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| {
                let path_str = entry.path().to_string_lossy();
                !path_str.contains("/.git/")
            })
        {
            if entry.file_type().is_file() {
                let source_path = entry.path().to_path_buf();
                let relative_path = source_path.strip_prefix(&dir_path).unwrap().to_path_buf();
                let target_path = home_path.join(relative_path);

                let dotfile = Dotfile {
                    source_path: source_path,
                    target_path: target_path.clone(),
                };

                // Later repos override earlier ones for the same file path
                filemap.insert(target_path, dotfile);
            }
        }
    }

    Ok(filemap)
}

pub fn fetch_modified(
    config: &Config,
    db: &Database,
    path: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let modified_dotfiles = get_modified_dotfiles(config, db, path)?;

    if modified_dotfiles.is_empty() {
        println!("{}", "No modified dotfiles to fetch.".green());
        return Ok(());
    }

    let grouped_by_repo = group_dotfiles_by_repo(&modified_dotfiles, config)?;

    print_fetch_plan(&grouped_by_repo, dry_run);

    if !dry_run {
        fetch_dotfiles(&modified_dotfiles, db, config.hash_cleanup_days)?;
    }

    Ok(())
}

fn get_modified_dotfiles(
    config: &Config,
    db: &Database,
    path: Option<&str>,
) -> Result<Vec<Dotfile>> {
    let all_dotfiles = get_all_dotfiles(config)?;
    let mut modified_dotfiles = Vec::new();

    if let Some(p) = path {
        let full_path = resolve_dotfile_path(p)?;

        for (target_path, dotfile) in all_dotfiles {
            if target_path.starts_with(&full_path) && dotfile.is_modified(db) {
                modified_dotfiles.push(dotfile);
            }
        }
    } else {
        for (_, dotfile) in all_dotfiles {
            if dotfile.is_modified(db) {
                modified_dotfiles.push(dotfile);
            }
        }
    }

    Ok(modified_dotfiles)
}

fn group_dotfiles_by_repo<'a>(
    dotfiles: &'a [Dotfile],
    config: &Config,
) -> Result<HashMap<String, Vec<&'a Dotfile>>> {
    let mut grouped_by_repo: HashMap<String, Vec<&Dotfile>> = HashMap::new();
    for dotfile in dotfiles {
        for repo in &config.repos {
            let local_repo = LocalRepo::new(config, repo.name.clone())?;
            if dotfile.source_path.starts_with(local_repo.local_path(config)?) {
                grouped_by_repo
                    .entry(repo.name.clone())
                    .or_default()
                    .push(dotfile);
                break;
            }
        }
    }
    Ok(grouped_by_repo)
}

fn print_fetch_plan(grouped_by_repo: &HashMap<String, Vec<&Dotfile>>, dry_run: bool) {
    if dry_run {
        println!(
            "{}",
            "Dry run: The following files would be fetched:".yellow()
        );
    } else {
        println!("{}", "Fetching the following modified files:".yellow());
    }

    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    for (repo_name, dotfiles) in grouped_by_repo {
        println!("  Repo: {}", repo_name.bold());
        for dotfile in dotfiles {
            let relative_path = dotfile.target_path.strip_prefix(&home).unwrap();
            println!("    - ~/{}", relative_path.display());
        }
    }
}

fn fetch_dotfiles(dotfiles: &[Dotfile], db: &Database, hash_cleanup_days: u32) -> Result<()> {
    for dotfile in dotfiles {
        dotfile.fetch(db)?;
    }
    db.cleanup_hashes(hash_cleanup_days)?;
    println!("\n{}", "Fetch complete.".green());
    Ok(())
}

pub fn apply_all(config: &Config, db: &Database) -> Result<()> {
    let filemap = get_all_dotfiles(config)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    for dotfile in filemap.values() {
        let was_missing = !dotfile.target_path.exists();
        dotfile.apply(&db)?;
        if was_missing {
            let relative = dotfile
                .target_path
                .strip_prefix(&home)
                .unwrap()
                .to_string_lossy();
            println!("Created new dotfile: ~/{}", relative);
        }
    }
    db.cleanup_hashes(config.hash_cleanup_days)?;
    Ok(())
}

pub fn reset_modified(config: &Config, db: &Database, path: &str) -> Result<()> {
    let filemap = get_all_dotfiles(config)?;
    let full_path = resolve_dotfile_path(path)?;
    for dotfile in filemap.values() {
        if dotfile.target_path.starts_with(&full_path) && dotfile.is_modified(&db) {
            dotfile.apply(&db)?;
        }
    }
    db.cleanup_hashes(config.hash_cleanup_days)?;
    Ok(())
}

/// List available subdirectories for a repository
pub fn list_repo_subdirs(config: &Config, repo_name: &str) -> Result<Vec<String>> {
    let _repo = find_repo_by_name(config, repo_name)?;
    let local_repo = localrepo::LocalRepo::new(config, repo_name.to_string())?;
    Ok(local_repo.meta.dots_dirs)
}

/// Set active subdirectories for a repository
pub fn set_repo_active_subdirs(
    config: &mut Config,
    repo_name: &str,
    subdirs: Vec<String>,
) -> Result<()> {
    let repo = find_repo_by_name(config, repo_name)?;

    // Validate that the subdirectories exist in the repo metadata
    let local_repo = localrepo::LocalRepo::new(config, repo_name.to_string())?;
    let meta = &local_repo.meta;

    for subdir in &subdirs {
        if !meta.dots_dirs.contains(subdir) {
            return Err(anyhow::anyhow!(
                "Subdirectory '{}' not found in repository. Available: {:?}",
                subdir,
                meta.dots_dirs
            ));
        }
    }

    config.set_active_subdirs(&repo.name, subdirs)?;
    Ok(())
}

/// Show active subdirectories for a repository
pub fn show_repo_active_subdirs(config: &Config, repo_name: &str) -> Result<Vec<String>> {
    let repo = find_repo_by_name(config, repo_name)?;

    let active_subdirs = config.get_active_subdirs(&repo.name);

    Ok(active_subdirs)
}

/// Find a repository by its name
fn find_repo_by_name(config: &Config, repo_name: &str) -> Result<config::Repo> {
    config
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", repo_name))
}

/// Prompt the user to select one of the configured repositories.
fn select_repo(config: &Config) -> Result<config::Repo> {
    use dialoguer::{Select, theme::ColorfulTheme};

    if config.repos.is_empty() {
        return Err(anyhow::anyhow!("No repositories configured"));
    }

    if config.repos.len() == 1 {
        return Ok(config.repos[0].clone());
    }

    let items: Vec<String> = config.repos.iter().map(|r| r.name.clone()).collect();
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select repository to add the dotfile to")
        .default(0)
        .items(&items)
        .interact()?;

    Ok(config.repos[selection].clone())
}

/// Prompt the user to select one of the repo's configured `dots_dirs`.
fn select_dots_dir(local_repo: &LocalRepo) -> Result<DotfileDir> {
    use dialoguer::{Select, theme::ColorfulTheme};

    let dirs = &local_repo.dotfile_dirs;

    if dirs.is_empty() {
        return Err(anyhow::anyhow!(
            "Repository '{}' has no configured dots_dirs",
            local_repo.name
        ));
    }

    if dirs.len() == 1 {
        return Ok(dirs[0].clone());
    }

    let items: Vec<String> = dirs
        .iter()
        .map(|d| {
            d.path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| d.path.display().to_string())
        })
        .collect();

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Select target dots_dir in repo '{}'",
            local_repo.name
        ))
        .default(0)
        .items(&items)
        .interact()?;

    Ok(dirs[selection].clone())
}

/// Add a new dotfile to tracking
pub fn add_dotfile(config: &Config, db: &Database, path: &str) -> Result<()> {
    // Resolve the path using git-style resolution
    let full_path = resolve_dotfile_path(path)?;

    // Repository selection
    let repo_config = select_repo(config)?;
    let local_repo = LocalRepo::new(config, repo_config.name.clone())?;

    // dots_dir selection
    let chosen_dir = select_dots_dir(&local_repo)?;

    // Construct destination path inside the repo
    let repo_base = local_repo.local_path(config)?;
    let dest_base = repo_base.join(&chosen_dir.path);

    // Compute relative path from home and final destination
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative = full_path.strip_prefix(&home).unwrap_or(&full_path);
    let dest_path = dest_base.join(relative);

    // Use Dotfile methods to perform the copy and DB registration
    let dotfile = Dotfile {
        source_path: dest_path.clone(),
        target_path: full_path.clone(),
    };
    // If the source already exists, treat as overwrite; Dotfile methods may be extended
    // later to prompt or handle conflicts more gracefully.
    dotfile.create_source_from_target(db)?;

    let chosen_dir_name = chosen_dir
        .path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| chosen_dir.path.display().to_string());

    println!(
        "Added {} to repo '{}' in directory '{}'",
        path, local_repo.name, chosen_dir_name
    );

    Ok(())
}

/// Remove a repository from configuration
pub fn remove_repo(config: &mut Config, repo_name: &str, remove_files: bool) -> Result<()> {
    let repo = find_repo_by_name(config, repo_name)?;

    // Safety check: ask for confirmation if removing files
    if remove_files {
        use dialoguer::Confirm;

        println!(
            "⚠️  {} {} {} {}",
            "WARNING:".red().bold(),
            "You are about to remove repository".red(),
            repo.name.green().bold(),
            "and all its local files!".red()
        );
        println!(
            "{}: {}",
            "Local path".yellow(),
            LocalRepo::new(config, repo_name.to_string())?
                .local_path(config)?
                .display()
        );

        let should_remove = Confirm::new()
            .with_prompt("Are you sure?")
            .default(false)
            .interact()?;

        if !should_remove {
            println!("{}", "Operation cancelled.".yellow());
            return Ok(());
        }
    }

    // Remove the repository from configuration
    config.repos.retain(|r| r.name != repo.name);
    config.save()?;

    // Optionally remove local files
    if remove_files {
        let local_repo = LocalRepo::new(config, repo_name.to_string())?;
        let local_path = local_repo.local_path(config)?;

        if local_path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&local_path) {
                eprintln!(
                    "{}: {}",
                    "Warning: failed to remove local files".yellow(),
                    e
                );
            } else {
                println!(
                    "{} {}",
                    "Removed local files:".green(),
                    local_path.display()
                );
            }
        }
    }

    Ok(())
}
