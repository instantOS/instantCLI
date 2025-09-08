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
pub mod utils;

pub use crate::dot::dotfile::Dotfile;
pub use git::{add_repo, status_all, update_all};

use crate::dot::config::{Config, repos_dir};
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use std::env::current_dir;
use std::fs;

/// Get a list of all active dotfile directory paths, ordered by repository relevance
pub fn get_active_dotfile_dirs(config: &Config) -> Result<Vec<PathBuf>> {
    let mut active_dirs = Vec::new();

    // Process repos in order of their configuration (relevance)
    for repo in &config.repos {
        let local_repo = match LocalRepo::new(config, repo.name.clone()) {
            Ok(repo) => repo,
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Warning: skipping repo '{}': {}", repo.name, e).yellow()
                );
                continue;
            }
        };

        for dotfile_dir in &local_repo.dotfile_dirs {
            if dotfile_dir.is_active {
                active_dirs.push(dotfile_dir.path.clone());
            }
        }
    }

    Ok(active_dirs)
}

/// Find the repository that contains the current working directory.
pub fn get_current_repo(config: &Config, cwd: &Path) -> Result<LocalRepo> {
    let mut this_repo: Option<LocalRepo> = None;
    for repo in &config.repos {
        let local = LocalRepo::new(config, repo.name.clone())?;
        if cwd.starts_with(local.local_path()?) {
            this_repo = Some(local);
            break;
        }
    }
    this_repo.ok_or_else(|| anyhow::anyhow!("Not in a dotfile repo"))
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

/// Fetch a single file from home directory to the repository
fn fetch_single_file(
    home_subdir: PathBuf,
    this_repo: &LocalRepo,
    db: &Database,
    config: &Config,
) -> Result<()> {
    if let Some(source_file) = this_repo.target_to_source(&home_subdir, config)? {
        fs::copy(&home_subdir, &source_file)?;
        let dotfile = Dotfile {
            source_path: source_file,
            target_path: home_subdir,
        };
        let _ = dotfile.get_source_hash(db);
    }
    Ok(())
}

/// Fetch files from a specific subdirectory
fn fetch_directory(path: &str, this_repo: &LocalRepo, db: &Database, home: &PathBuf) -> Result<()> {
    let dotfiles = this_repo.get_all_dotfiles()?;
    let relative_path = path.trim_start_matches('/');
    let target_path = home.join(relative_path);

    for dotfile in dotfiles.values() {
        if dotfile.target_path.starts_with(&target_path) && dotfile.target_path.exists() {
            dotfile.fetch(db)?;
        }
    }
    Ok(())
}

/// Fetch all tracked files globally
fn fetch_all_files(this_repo: &LocalRepo, db: &Database, _home: &PathBuf) -> Result<()> {
    let dotfiles = this_repo.get_all_dotfiles()?;

    for dotfile in dotfiles.values() {
        if dotfile.target_path.exists() {
            dotfile.fetch(db)?;
        }
    }
    Ok(())
}

/// Fetch modified files from home directory back to the repository
pub fn fetch_modified(config: &Config, path: Option<&str>) -> Result<()> {
    let cwd = current_dir()?;
    let db = Database::new()?;
    let this_repo = get_current_repo(config, &cwd)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    if let Some(p) = path {
        let p = p.trim_start_matches('.');
        let home_subdir = home.join(p);
        if !home_subdir.exists() {
            return Ok(());
        }

        let md = fs::metadata(&home_subdir)?;
        if md.is_file() {
            fetch_single_file(home_subdir, &this_repo, &db, config)?;
        } else if md.is_dir() {
            fetch_directory(p, &this_repo, &db, &home)?
        }
    } else {
        // Global fetch: get all dotfiles from the current repo and fetch tracked
        fetch_all_files(&this_repo, &db, &home)?;
    }
    db.cleanup_hashes()?;
    Ok(())
}

pub fn apply_all(config: &Config) -> Result<()> {
    let db = Database::new()?;
    let filemap = get_all_dotfiles(config)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    for dotfile in filemap.values() {
        let was_missing = !dotfile.target_path.exists();
        dotfile.apply(&db)?;
        if was_missing {
            let relative = dotfile.target_path.strip_prefix(&home).unwrap().to_string_lossy();
            println!("Created new dotfile: ~/{}", relative);
        }
    }
    db.cleanup_hashes()?;
    Ok(())
}

pub fn reset_modified(config: &Config, path: &str) -> Result<()> {
    let db = Database::new()?;
    let filemap = get_all_dotfiles(config)?;
    let expanded = shellexpand::tilde(path).into_owned();
    let full_path = PathBuf::from(expanded);
    for dotfile in filemap.values() {
        if dotfile.target_path.starts_with(&full_path) && dotfile.is_modified(&db) {
            dotfile.apply(&db)?;
        }
    }
    db.cleanup_hashes()?;
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

    config.set_active_subdirs(&repo.url, subdirs)?;
    Ok(())
}

/// Show active subdirectories for a repository
pub fn show_repo_active_subdirs(config: &Config, repo_name: &str) -> Result<Vec<String>> {
    let repo = find_repo_by_name(config, repo_name)?;

    let active_subdirs = config.get_active_subdirs(&repo.url);

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

/// Add a new dotfile to tracking
pub fn add_dotfile(config: &Config, path: &str) -> Result<()> {
    let cwd = current_dir()?;
    let db = Database::new()?;
    let this_repo = get_current_repo(config, &cwd)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    let expanded = shellexpand::tilde(path).into_owned();
    let full_path = PathBuf::from(expanded);

    if !full_path.exists() {
        return Err(anyhow::anyhow!("File '{}' does not exist", path));
    }

    // Check if the file is in the home directory
    if !full_path.starts_with(&home) {
        return Err(anyhow::anyhow!("File '{}' is not in home directory", path));
    }

    // Find the corresponding source path in the repo
    if let Some(source_path) = this_repo.target_to_source(&full_path, config)? {
        // Copy the file to the repo
        if let Some(parent) = source_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&full_path, &source_path)?;

        // Create dotfile and compute hash
        let dotfile = Dotfile {
            source_path: source_path,
            target_path: full_path,
        };
        let _ = dotfile.get_source_hash(&db);

        println!("Added {} to tracking", path);
    } else {
        return Err(anyhow::anyhow!(
            "No matching source directory found for '{}'",
            path
        ));
    }

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
                .local_path()?
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
        let local_path = local_repo.local_path()?;

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
