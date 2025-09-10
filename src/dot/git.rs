use crate::dot::config;
use crate::dot::localrepo as repo_mod;
use crate::dot::utils;
use anyhow::{Context, Result};
use colored::*;
use std::{path::PathBuf, process::Command};
use crate::dot::get_all_dotfiles;

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

    let mut cmd = Command::new("git");
    cmd.arg("clone");
    if depth > 0 {
        cmd.arg("--depth").arg(depth.to_string());
    }
    if let Some(branch) = &repo.branch {
        cmd.arg("--branch").arg(branch);
    }
    cmd.arg(&repo.url).arg(&target);

    if debug {
        eprintln!("Running: {:?}", cmd);
    }

    // Create progress bar for cloning operation
    let pb = utils::create_spinner(format!("Cloning {}...", repo.url));

    let output = cmd.output().context("running git clone")?;
    pb.finish_with_message(format!("Cloned {}", repo.url));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "git clone failed for {}: {}",
            repo.url,
            stderr
        ));
    }

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
    if let Ok(db) = crate::dot::db::Database::new(config_manager.config.database_path().to_path_buf()) {
        if let Ok(dotfiles) = get_all_dotfiles(&config_manager.config, &db) {
            for (_, dotfile) in dotfiles {
                // Only register hashes for dotfiles from this repository
                if dotfile.source_path.starts_with(&target) {
                    // Register the source file hash as unmodified
                    if let Ok(source_hash) = crate::dot::dotfile::Dotfile::compute_hash(&dotfile.source_path) {
                        db.add_hash(&source_hash, &dotfile.source_path, true)?;
                        
                        // If the target file exists and has the same content, 
                        // register it as unmodified too
                        if dotfile.target_path.exists() {
                            if let Ok(target_hash) = crate::dot::dotfile::Dotfile::compute_hash(&dotfile.target_path) {
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
) -> Result<()> {
    let all_dotfiles = super::get_all_dotfiles(cfg, db)?;
    let home = std::path::PathBuf::from(shellexpand::tilde("~").to_string());
    
    if let Some(path_str) = path {
        // Show status for specific path
        let target_path = super::resolve_dotfile_path(path_str)?;
        
        if let Some(dotfile) = all_dotfiles.get(&target_path) {
            println!("{} -> {}", target_path.display(), get_dotfile_status_string(dotfile, db));
            println!("  Source: {}", dotfile.source_path.display());
            println!("  Repo: {}", get_repo_name_for_dotfile(&dotfile, cfg));
        } else {
            println!("{} -> not tracked", target_path.display());
        }
    } else {
        // Show status for all dotfiles
        let mut has_modified = false;
        let mut has_outdated = false;
        let mut has_clean = false;
        
        for (target_path, dotfile) in all_dotfiles {
            let status = get_dotfile_status_string(&dotfile, db);
            let relative_path = target_path.strip_prefix(&home)
                .unwrap_or(&target_path);
            
            match status.as_str() {
                "modified" => has_modified = true,
                "outdated" => has_outdated = true,
                "clean" => has_clean = true,
                _ => {}
            }
            
            // Only show non-clean files by default
            if status != "clean" {
                println!("~{} -> {} ({})", 
                    relative_path.display(), 
                    status, 
                    get_repo_name_for_dotfile(&dotfile, cfg)
                );
            }
        }
        
        // Show summary
        if !has_modified && !has_outdated {
            if has_clean {
                println!("All dotfiles are clean.");
            } else {
                println!("No dotfiles found.");
            }
        } else {
            if has_modified {
                println!("\n{} dotfile(s) modified", "modified".yellow());
            }
            if has_outdated {
                println!("{} dotfile(s) outdated", "outdated".blue());
            }
        }
    }
    
    Ok(())
}

fn get_dotfile_status_string(dotfile: &super::Dotfile, db: &super::db::Database) -> String {
    if dotfile.is_modified(db) {
        "modified".yellow().to_string()
    } else if dotfile.is_outdated(db) {
        "outdated".blue().to_string()
    } else {
        "clean".green().to_string()
    }
}

fn get_repo_name_for_dotfile(dotfile: &super::Dotfile, cfg: &config::Config) -> String {
    // Find which repository this dotfile comes from
    for repo_config in &cfg.repos {
        if dotfile.source_path.starts_with(&cfg.repos_path().join(&repo_config.name)) {
            return repo_config.name.clone();
        }
    }
    "unknown".to_string()
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
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .arg("status")
                    .arg("--porcelain")
                    .output()
                    .with_context(|| format!("running git status in {}", target.display()))?;
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

            let output = Command::new("git")
                .arg("-C")
                .arg(&target)
                .arg("status")
                .arg("--porcelain")
                .output()
                .with_context(|| format!("running git status in {}", target.display()))?;

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
