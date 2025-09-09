use anyhow::Result;
use colored::*;
use shellexpand;
use std::collections::HashMap;
use std::path::PathBuf;

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
        fetch_dotfiles(&modified_dotfiles, db)?;
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
        let expanded = shellexpand::tilde(p).into_owned();
        let full_path = PathBuf::from(expanded);

        if !full_path.exists() {
            return Err(anyhow::anyhow!("Path does not exist: {}", p));
        }

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
            if dotfile.source_path.starts_with(local_repo.local_path()?) {
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
        println!("{}", "Dry run: The following files would be fetched:".yellow());
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

fn fetch_dotfiles(dotfiles: &[Dotfile], db: &Database) -> Result<()> {
    for dotfile in dotfiles {
        dotfile.fetch(db)?;
    }
    db.cleanup_hashes()?;
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
    db.cleanup_hashes()?;
    Ok(())
}

pub fn reset_modified(config: &Config, db: &Database, path: &str) -> Result<()> {
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
pub fn add_dotfile(config: &Config, db: &Database, path: &str) -> Result<()> {
    let this_repo_config = config.repos.last().ok_or_else(|| anyhow::anyhow!("No repositories configured"))?;
    let this_repo = LocalRepo::new(config, this_repo_config.name.clone())?;
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
            "No matching source directory found for '{}' in repo '{}'",
            path,
            this_repo.name
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
