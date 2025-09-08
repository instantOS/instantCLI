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

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use std::env::current_dir;
use std::fs;

/// Find the repository that contains the current working directory.
pub fn get_current_repo(config: &Config, cwd: &Path) -> Result<LocalRepo> {
    let mut this_repo: Option<LocalRepo> = None;
    for repo in &config.repos {
        let local = LocalRepo::from(repo.clone());
        if cwd.starts_with(local.local_path()?) {
            this_repo = Some(local);
            break;
        }
    }
    this_repo.ok_or(anyhow::anyhow!("Not in a dotfile repo"))
}

pub fn get_all_dotfiles() -> Result<HashMap<PathBuf, Dotfile>> {
    let mut filemap = HashMap::new();
    let config = config::Config::load()?;
    let repos = config.repos;
    let base_dir = config::repos_base_dir()?;

    for repo in repos {
        let repo_name = repo.name.clone();
        let repo_path = base_dir.join(repo_name);

        // Get active subdirectories for this repo (defaults to ["dots"])
        let active_subdirs = repo.active_subdirs.clone();

        // Read repo metadata to get available dots directories
        let meta = match crate::dot::localrepo::LocalRepo::from(repo.clone()).read_meta() {
            Ok(meta) => meta,
            Err(_) => {
                // If metadata is invalid, skip this repo
                continue;
            }
        };

        // Process each active subdirectory
        for subdir in active_subdirs {
            // Check if this subdirectory exists in the repo's metadata
            if !meta.dots_dirs.contains(&subdir) {
                continue;
            }

            let dots_path = repo_path.join(&subdir);
            if !dots_path.exists() {
                continue;
            }

            for entry in WalkDir::new(&dots_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|entry| {
                    let path_str = entry.path().to_string_lossy();
                    !path_str.contains("/.git/")
                })
            {
                if entry.file_type().is_file() {
                    let source_path = entry.path().to_path_buf();
                    let relative_path = source_path.strip_prefix(&dots_path).unwrap().to_path_buf();
                    let target_path =
                        PathBuf::from(shellexpand::tilde("~").to_string()).join(relative_path);

                    let dotfile = Dotfile {
                        repo_path: source_path,
                        target_path: target_path.clone(),
                        hash: None,
                        target_hash: None,
                    };

                    // Later repos override earlier ones for the same file path
                    filemap.insert(target_path, dotfile);
                }
            }
        }
    }

    Ok(filemap)
}

/// Fetch a single file from home directory to the repository
fn fetch_single_file(home_subdir: PathBuf, this_repo: &LocalRepo, db: &Database) -> Result<()> {
    if let Some(source_file) = this_repo.target_to_source(&home_subdir)? {
        fs::copy(&home_subdir, &source_file)?;
        let dotfile = Dotfile {
            repo_path: source_file,
            target_path: home_subdir,
            hash: None,
            target_hash: None,
        };
        let _ = dotfile.get_source_hash(db);
    }
    Ok(())
}

/// Fetch files from a specific subdirectory
fn fetch_directory(path: &str, this_repo: &LocalRepo, db: &Database, home: &PathBuf) -> Result<()> {
    let active_dirs = this_repo.get_active_dots_dirs()?;
    let relative_path = path.trim_start_matches('/');

    for dots_dir in active_dirs {
        let source_subdir = dots_dir.join(relative_path);
        if source_subdir.exists() {
            // Walk existing source subdir and fetch tracked
            for entry in WalkDir::new(&source_subdir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let source_file = entry.path().to_path_buf();
                let relative = source_file.strip_prefix(&dots_dir).unwrap().to_path_buf();
                let target_file = home.join(relative);
                if target_file.exists() {
                    let dotfile = Dotfile {
                        repo_path: source_file,
                        target_path: target_file,
                        hash: None,
                        target_hash: None,
                    };
                    dotfile.fetch(db)?;
                }
            }
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
pub fn fetch_modified(path: Option<&str>) -> Result<()> {
    let cwd = current_dir()?;
    let db = Database::new()?;
    let config = Config::load()?;
    let this_repo = get_current_repo(&config, &cwd)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    if let Some(p) = path {
        let p = p.trim_start_matches('.');
        let home_subdir = home.join(p);
        if !home_subdir.exists() {
            return Ok(());
        }

        let md = fs::metadata(&home_subdir)?;
        if md.is_file() {
            fetch_single_file(home_subdir, &this_repo, &db)?;
        } else if md.is_dir() {
            fetch_directory(p, &this_repo, &db, &home)?;
        }
    } else {
        // Global fetch: get all dotfiles from the current repo and fetch tracked
        fetch_all_files(&this_repo, &db, &home)?;
    }
    db.cleanup_hashes()?;
    Ok(())
}

pub fn apply_all() -> Result<()> {
    let db = Database::new()?;
    let filemap = get_all_dotfiles()?;
    for dotfile in filemap.values() {
        dotfile.apply(&db)?;
    }
    db.cleanup_hashes()?;
    Ok(())
}

pub fn reset_modified(path: &str) -> Result<()> {
    let db = Database::new()?;
    let filemap = get_all_dotfiles()?;
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
pub fn list_repo_subdirs(repo_identifier: &str) -> Result<Vec<String>> {
    let config = Config::load()?;
    let repo = find_repo_by_identifier(&config, repo_identifier)?;
    let local_repo = localrepo::LocalRepo::from(repo);
    let meta = local_repo.read_meta()?;
    Ok(meta.dots_dirs)
}

/// Set active subdirectories for a repository
pub fn set_repo_active_subdirs(repo_identifier: &str, subdirs: Vec<String>) -> Result<()> {
    let mut config = Config::load()?;
    let repo = find_repo_by_identifier(&config, repo_identifier)?;

    // Validate that the subdirectories exist in the repo metadata
    let local_repo = localrepo::LocalRepo::from(repo.clone());
    let meta = local_repo.read_meta()?;

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
pub fn show_repo_active_subdirs(repo_identifier: &str) -> Result<Vec<String>> {
    let config = Config::load()?;
    let repo = find_repo_by_identifier(&config, repo_identifier)?;

    let active_subdirs = config
        .get_active_subdirs(&repo.url)
        .unwrap_or_else(|| vec!["dots".to_string()]);

    Ok(active_subdirs)
}

/// Helper function to find a repository by name
// TODO: come up with better name for this
fn find_repo_by_identifier(config: &Config, identifier: &str) -> Result<config::Repo> {
    config
        .repos
        .iter()
        .find(|r| r.name == identifier)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", identifier))
}

/// Remove a repository from configuration
pub fn remove_repo(repo_identifier: &str, remove_files: bool) -> Result<()> {
    let mut config = Config::load()?;
    let repo = find_repo_by_identifier(&config, repo_identifier)?;

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
            LocalRepo::from(repo.clone()).local_path()?.display()
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
        let local_repo = LocalRepo::from(repo);
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
