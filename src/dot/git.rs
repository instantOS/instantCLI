use crate::dot::config;
use crate::dot::get_all_dotfiles;
use crate::dot::localrepo as repo_mod;
use crate::dot::utils;
use anyhow::{Context, Result};
use colored::*;
use std::path::PathBuf;
use std::process::Command;

pub fn add_repo(
    config_manager: &mut config::ConfigManager,
    repo: config::Repo,
    debug: bool,
) -> Result<PathBuf> {
    let base = config_manager.config.repos_path();

    let repo_dir_name = repo.name.clone();

    let target = base.join(repo_dir_name);

    if target.exists() {
        return Err(anyhow::anyhow!(
            "Destination '{}' already exists",
            target.display()
        ));
    }

    let depth = config_manager.config.clone_depth;

    let pb = utils::create_spinner(format!("Cloning {}...", repo.url));

    utils::git_clone(
        &repo.url,
        &target,
        repo.branch.as_deref(),
        depth as i32,
        debug,
    )?;

    pb.finish_with_message(format!("Cloned {}", repo.url));

    // Note: config addition is now handled by the caller (add_repository function)

    // validate metadata but do not delete invalid clones; report their existence
    let local_repo = repo_mod::LocalRepo::new(&config_manager.config, repo.name.clone())?;
    let meta = &local_repo.meta;

    if debug {
        eprintln!(
            "Repo {} identified as dot repo '{}' - {}",
            local_repo.url,
            meta.name,
            meta.description.as_deref().unwrap_or("")
        );
    }

    // Initialize database with source file hashes to prevent false "modified" status
    // when identical files already exist in the home directory
    if let Ok(db) =
        crate::dot::db::Database::new(config_manager.config.database_path().to_path_buf())
    {
        if let Ok(dotfiles) = get_all_dotfiles(&config_manager.config, &db) {
            for (_, dotfile) in dotfiles {
                // Only register hashes for dotfiles from this repository
                if dotfile.source_path.starts_with(&target) {
                    // Register the source file hash as unmodified
                    if let Ok(source_hash) =
                        crate::dot::dotfile::Dotfile::compute_hash(&dotfile.source_path)
                    {
                        db.add_hash(&source_hash, &dotfile.source_path, true)?;

                        // If the target file exists and has the same content,
                        // register it as unmodified too
                        if dotfile.target_path.exists() {
                            if let Ok(target_hash) =
                                crate::dot::dotfile::Dotfile::compute_hash(&dotfile.target_path)
                            {
                                if target_hash == source_hash {
                                    db.add_hash(&target_hash, &dotfile.target_path, true)?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(target)
}

pub fn update_all(cfg: &config::Config, debug: bool) -> Result<()> {
    let repos = cfg.repos.clone();
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    let mut any_failed = false;

    for repo in repos.iter() {
        let local_repo = repo_mod::LocalRepo::new(cfg, repo.name.clone())?;
        if let Err(e) = local_repo.update(cfg, debug) {
            eprintln!("Failed to update {}: {}", repo.url, e);
            any_failed = true;
        }
    }

    if any_failed {
        Err(anyhow::anyhow!(
            "One or more repositories failed to update (see error messages above for details)"
        ))
    } else {
        Ok(())
    }
}

pub fn status_all(
    cfg: &config::Config,
    debug: bool,
    path: Option<&str>,
    db: &super::db::Database,
    show_all: bool,
) -> Result<()> {
    let all_dotfiles = super::get_all_dotfiles(cfg, db)?;

    if let Some(path_str) = path {
        // Show status for specific path
        show_single_file_status(path_str, &all_dotfiles, cfg, db)?;
    } else {
        // Show summary and file list
        show_status_summary(&all_dotfiles, cfg, db, show_all)?;
    }

    Ok(())
}

fn show_single_file_status(
    path_str: &str,
    all_dotfiles: &std::collections::HashMap<PathBuf, super::Dotfile>,
    cfg: &config::Config,
    db: &super::db::Database,
) -> Result<()> {
    let target_path = super::resolve_dotfile_path(path_str)?;

    if let Some(dotfile) = all_dotfiles.get(&target_path) {
        let repo_name = get_repo_name_for_dotfile(&dotfile, cfg);
        let dotfile_dir = get_dotfile_dir_name(&dotfile, cfg);
        println!(
            "{} -> {}",
            target_path.display(),
            get_dotfile_status(dotfile, db)
        );
        println!("  Source: {}", dotfile.source_path.display());
        println!("  Repo: {} ({})", repo_name, dotfile_dir);
    } else {
        println!("{} -> not tracked", target_path.display());
    }

    Ok(())
}

fn show_status_summary(
    all_dotfiles: &std::collections::HashMap<PathBuf, super::Dotfile>,
    cfg: &config::Config,
    db: &super::db::Database,
    show_all: bool,
) -> Result<()> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let mut files_by_status = std::collections::HashMap::new();
    let mut repo_stats = std::collections::HashMap::new();

    // Categorize files by status and collect repo statistics
    // TODO: extract this into a separate function
    for (target_path, dotfile) in all_dotfiles {
        let status = get_dotfile_status(&dotfile, db);
        let repo_name = get_repo_name_for_dotfile(&dotfile, cfg);
        let dotfile_dir = get_dotfile_dir_name(&dotfile, cfg);

        // Store file info for later display
        files_by_status
            .entry(status.clone())
            .or_insert_with(Vec::new)
            .push((
                target_path.clone(),
                dotfile.clone(),
                repo_name.clone(),
                dotfile_dir.clone(),
            ));

        // Update repo statistics
        let repo_entry = repo_stats
            .entry(repo_name.clone())
            .or_insert_with(|| std::collections::HashMap::new());
        *repo_entry.entry(dotfile_dir.clone()).or_insert(0) += 1;
    }

    let total_files = all_dotfiles.len();
    let clean_count = files_by_status
        .get(&DotFileStatus::Clean)
        .map_or(0, |v| v.len());
    let modified_count = files_by_status
        .get(&DotFileStatus::Modified)
        .map_or(0, |v| v.len());
    let outdated_count = files_by_status
        .get(&DotFileStatus::Outdated)
        .map_or(0, |v| v.len());

    println!("Total tracked: {} files", total_files);
    println!("{} Clean: {} files", "✓".green(), clean_count);

    if modified_count > 0 {
        println!("{} Modified: {} files", "⚠".yellow(), modified_count);
    }

    if outdated_count > 0 {
        println!("{} Outdated: {} files", "↓".blue(), outdated_count);
    }

    // Show files with issues
    if modified_count > 0 || outdated_count > 0 {
        println!();

        if let Some(modified_files) = files_by_status.get(&DotFileStatus::Modified) {
            println!("{}", "Modified files:".yellow().bold());
            for (target_path, dotfile, repo_name, dotfile_dir) in modified_files {
                let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
                let tilde_path = format!("~/{}", relative_path.display());
                println!(
                    "  {} -> {} ({}: {})",
                    tilde_path,
                    "modified".yellow(),
                    repo_name,
                    dotfile_dir
                );
            }
            println!();
        }

        if let Some(outdated_files) = files_by_status.get(&DotFileStatus::Outdated) {
            println!("{}", "Outdated files:".blue().bold());
            for (target_path, dotfile, repo_name, dotfile_dir) in outdated_files {
                let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
                let tilde_path = format!("~/{}", relative_path.display());
                println!(
                    "  {} -> {} ({}: {})",
                    tilde_path,
                    "outdated".blue(),
                    repo_name,
                    dotfile_dir
                );
            }
            println!();
        }
    }

    // Show all files if requested
    if show_all && clean_count > 0 {
        println!("{}", "Clean files:".green().bold());
        for (target_path, dotfile, repo_name, dotfile_dir) in files_by_status
            .get(&DotFileStatus::Clean)
            .unwrap_or(&vec![])
        {
            let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
            let tilde_path = format!("~/{}", relative_path.display());
            println!(
                "  {} -> {} ({}: {})",
                tilde_path,
                "clean".green(),
                repo_name,
                dotfile_dir
            );
        }
        println!();
    }

    // Show action suggestions
    // TODO: extract this into a separate function
    if modified_count > 0 || outdated_count > 0 {
        println!("{}", "Suggested actions:".bold());
        if modified_count > 0 {
            println!("  Use 'instant dot apply' to apply changes from repositories");
            println!("  Use 'instant dot fetch' to save your modifications to repositories");
        }
        if outdated_count > 0 {
            println!("  Use 'instant dot reset <path>' to restore files to their original state");
        }
        println!("  Use 'instant dot status --all' to see all tracked files including clean ones");
    } else if clean_count > 0 {
        println!("✓ All dotfiles are clean and up to date!");
    } else {
        println!("No dotfiles found. Use 'instant dot repo add <url>' to add a repository.");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
enum DotFileStatus {
    Modified,
    Outdated,
    Clean,
}

impl std::fmt::Display for DotFileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DotFileStatus::Modified => write!(f, "{}", "modified".yellow()),
            DotFileStatus::Outdated => write!(f, "{}", "outdated".blue()),
            DotFileStatus::Clean => write!(f, "{}", "clean".green()),
        }
    }
}

fn get_dotfile_status(dotfile: &super::Dotfile, db: &super::db::Database) -> DotFileStatus {
    if dotfile.is_modified(db) {
        DotFileStatus::Modified
    } else if dotfile.is_outdated(db) {
        DotFileStatus::Outdated
    } else {
        DotFileStatus::Clean
    }
}

// Get the dotfile directory name for a dotfile
fn get_dotfile_dir_name(dotfile: &super::Dotfile, cfg: &config::Config) -> String {
    // Find which repository this dotfile comes from
    for repo_config in &cfg.repos {
        let repo_path = cfg.repos_path().join(&repo_config.name);
        if dotfile.source_path.starts_with(&repo_path) {
            // Extract the dotfile directory name from the source path
            // Source path format: {repo_path}/{dotfile_dir}/{relative_path}
            if let Ok(relative) = dotfile.source_path.strip_prefix(&repo_path) {
                if let Some(dotfile_dir) = relative.components().next() {
                    return dotfile_dir.as_os_str().to_string_lossy().to_string();
                }
            }
            return "dots".to_string(); // default
        }
    }
    "unknown".to_string()
}

// Get the repository name for a dotfile (improved version)
fn get_repo_name_for_dotfile(dotfile: &super::Dotfile, cfg: &config::Config) -> super::RepoName {
    // Find which repository this dotfile comes from
    for repo_config in &cfg.repos {
        if dotfile
            .source_path
            .starts_with(&cfg.repos_path().join(&repo_config.name))
        {
            return super::RepoName::new(repo_config.name.clone());
        }
    }
    super::RepoName::new("unknown".to_string())
}

// Legacy status function - kept for compatibility but should be removed
pub fn status_all_legacy(
    cfg: &config::Config,
    debug: bool,
    path: Option<&str>,
    db: &super::db::Database,
) -> Result<()> {
    let repos = cfg.repos.clone();
    let base = cfg.repos_path();
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    let query = path.map(|s| s.to_string());
    let mut found = false;

    for crepo in repos.iter() {
        let repo_dir_name = crepo.name.clone();
        let target = base.join(repo_dir_name);

        if !target.exists() {
            println!("{} -> {}", crepo.url.bold(), "missing".red());
            continue;
        }

        // validate instantdots.toml exists and parse it via LocalRepo
        let local = repo_mod::LocalRepo::new(cfg, crepo.name.clone())?;
        let meta = &local.meta;

        if debug {
            eprintln!(
                "Repo {} identified as dot repo '{}' - {}",
                crepo.url,
                meta.name,
                meta.description.as_deref().unwrap_or("")
            );
        }

        let _branch = match &crepo.branch {
            Some(b) => b.clone(),
            None => "(no branch configured)".to_string(),
        };

        // get checked out branch
        let _current_branch = match local.get_checked_out_branch(cfg) {
            Ok(b) => b,
            Err(e) => {
                println!("{} -> cannot determine branch: {}", crepo.url, e);
                continue;
            }
        };

        // If a specific path was provided, check whether it belongs to this repo and report
        if let Some(p) = path {
            // Use the new path resolution function
            let provided = match super::resolve_dotfile_path(p) {
                Ok(path) => path,
                Err(e) => {
                    // If path resolution fails, show error but continue with other repos
                    eprintln!("Error resolving path '{}': {}", p, e);
                    continue;
                }
            };

            // If this repo contains the provided dotfile, print its status and repo info
            // Determine if this target path maps to a source under this repo's dots/
            let dots_dir = target.join("dots");
            let rel = match provided.strip_prefix(shellexpand::tilde("~").into_owned()) {
                Ok(r) => r.to_path_buf(),
                Err(_) => provided.clone(),
            };
            let source_candidate = dots_dir.join(&rel);

            if source_candidate.exists() && source_candidate.starts_with(&target) {
                found = true;
                println!("File: {}", provided.display());
                println!("Repo: {}", crepo.url);

                // git status for repo
                let output = utils::git_command_in_dir_with_output(
                    &target,
                    &["status", "--porcelain"],
                    "status",
                )?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim().is_empty() {
                    println!("Repo status: {}", "clean".green());
                } else {
                    println!("Repo status: {}\n{}", "modified".yellow(), stdout);
                }

                // now check file status using db
                let filemap = super::get_all_dotfiles(cfg, db)?;
                if let Some(dotfile) = filemap.get(&provided) {
                    println!("Source: {}", dotfile.source_path.display());
                    if dotfile.is_modified(&db) {
                        println!("File status: {}", "modified".yellow());
                    } else if dotfile.is_outdated(&db) {
                        println!("File status: {}", "outdated".blue());
                    } else {
                        println!("File status: {}", "clean".green());
                    }
                } else {
                    println!("File not tracked by instantdots in this repo.");
                }
            }
        } else {
            // No specific path: show repo-level and per-file summaries as before

            let output = utils::git_command_in_dir_with_output(
                &target,
                &["status", "--porcelain"],
                "status",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                println!("{} -> {}", crepo.url.bold(), "clean".green());
            } else {
                println!(
                    "{} -> {}
{}",
                    crepo.url.bold(),
                    "modified".yellow(),
                    stdout
                );
            }

            // Now check individual dotfile statuses for this repo
            let filemap = super::get_all_dotfiles(cfg, db)?;

            for (target_path, dotfile) in filemap.iter() {
                // Only show dotfiles belonging to the current repo
                if dotfile.source_path.starts_with(&target) {
                    if dotfile.is_modified(&db) {
                        println!(
                            "    {} -> {}",
                            target_path.to_string_lossy().bold(),
                            "modified".yellow()
                        );
                    } else if dotfile.is_outdated(&db) {
                        println!(
                            "    {} -> {}",
                            target_path.to_string_lossy().bold(),
                            "outdated".blue()
                        );
                    } else {
                        println!(
                            "    {} -> {}",
                            target_path.to_string_lossy().bold(),
                            "clean".green()
                        );
                    }
                }
            }
        }
    }

    if query.is_some() && !found {
        println!("{} -> not found in any configured repo.", query.unwrap());
    }

    Ok(())
}
