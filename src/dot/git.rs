use crate::dot::config;
use crate::dot::localrepo as repo_mod;
use crate::dot::utils;
use anyhow::{Context, Result};
use colored::*;
use std::{path::PathBuf, process::Command};

pub fn add_repo(cfg: &mut config::Config, repo: config::Repo, debug: bool) -> Result<PathBuf> {
    let base = config::repos_base_dir()?;

    let repo_dir_name = repo.name.clone();

    let target = base.join(repo_dir_name);

    if target.exists() {
        return Err(anyhow::anyhow!(
            "Destination '{}' already exists",
            target.display()
        ));
    }

    let depth = cfg.clone_depth;

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
        return Err(anyhow::anyhow!("git clone failed: {}", stderr));
    }

    // append to config
    let local: repo_mod::LocalRepo = repo.clone().into();
    cfg.add_repo(repo)?;

    // validate metadata but do not delete invalid clones; report their existence
    match local.read_meta() {
        Ok(meta) => {
            if debug {
                eprintln!(
                    "Repo {} identified as dot repo '{}' - {}",
                    local.url,
                    meta.name,
                    meta.description.as_deref().unwrap_or("")
                );
            }
        }
        Err(e) => {
            if debug {
                eprintln!("{} -> not a valid instantdots repo: {}", local.url, e);
            } else {
                println!("{} -> not a valid instantdots repo: {}", local.url, e);
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

    for crepo in repos.iter() {
        let local: repo_mod::LocalRepo = crepo.clone().into();
        match local.read_meta() {
            Ok(_) => {
                if let Err(e) = local.update(debug) {
                    eprintln!("Failed to update {}: {}", crepo.url, e);
                    any_failed = true;
                }
            }
            Err(_e) => {
                println!(
                    "{} -> {}",
                    crepo.url.bold(),
                    "not a valid instantdots repo".red()
                );
                continue;
            }
        }
    }

    if any_failed {
        Err(anyhow::anyhow!("One or more repos failed to update"))
    } else {
        Ok(())
    }
}

pub fn status_all(cfg: &config::Config, debug: bool, path: Option<&str>) -> Result<()> {
    let repos = cfg.repos.clone();
    let base = config::repos_base_dir()?;
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
        let local: repo_mod::LocalRepo = crepo.clone().into();
        match local.read_meta() {
            Ok(meta) => {
                if debug {
                    eprintln!(
                        "Repo {} identified as dot repo '{}' - {}",
                        crepo.url,
                        meta.name,
                        meta.description.as_deref().unwrap_or("")
                    );
                }
            }
            Err(_e) => {
                println!(
                    "{} -> {}",
                    crepo.url.bold(),
                    "not a valid instantdots repo".red()
                );
                continue;
            }
        }

        let _branch = match &crepo.branch {
            Some(b) => b.clone(),
            None => "(no branch configured)".to_string(),
        };

        // get checked out branch
        let _current_branch = match local.get_branch() {
            Ok(b) => b,
            Err(e) => {
                println!("{} -> cannot determine branch: {}", crepo.url, e);
                continue;
            }
        };

        // If a specific path was provided, check whether it belongs to this repo and report
        if let Some(p) = path {
            // Normalize provided path (expand ~ and make absolute-ish)
            let expanded = shellexpand::tilde(p).into_owned();
            let provided = PathBuf::from(expanded);

            // If this repo contains the provided dotfile, print its status and repo info
            if provided.exists() {
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
                    let db = super::db::Database::new()?;
                    let filemap = super::get_all_dotfiles(cfg)?;
                    if let Some(dotfile) = filemap.get(&provided) {
                        println!("Source: {}", dotfile.source_path.display());
                        if dotfile.is_modified(&db) {
                            println!("File status: {}", "modified".yellow());
                        } else if dotfile.is_outdated() {
                            println!("File status: {}", "outdated".blue());
                        } else {
                            println!("File status: {}", "clean".green());
                        }
                    } else {
                        println!("File not tracked by instantdots in this repo.");
                    }
                }
            } else {
                // Provided path doesn't exist; still attempt to map to repo source
                let dots_dir = target.join("dots");
                // Make provided relative path by trimming leading ~/
                let rel = p.trim_start_matches("~").trim_start_matches('/');
                let source_candidate = dots_dir.join(rel);
                if source_candidate.exists() {
                    println!("File: {}", p);
                    println!("Repo: {}", crepo.url);
                    println!("Source: {}", source_candidate.display());
                    let db = super::db::Database::new()?;
                    let dotfile = super::Dotfile {
                        source_path: source_candidate.clone(),
                        target_path: PathBuf::from(shellexpand::tilde("~").to_string()).join(rel),
                        hash: None,
                        target_hash: None,
                    };
                    if dotfile.is_modified(&db) {
                        println!("File status: {}", "modified".yellow());
                    } else if dotfile.is_outdated() {
                        println!("File status: {}", "outdated".blue());
                    } else {
                        println!("File status: {}", "clean".green());
                    }
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
            let db = super::db::Database::new()?;
            let filemap = super::get_all_dotfiles(cfg)?;

            for (target_path, dotfile) in filemap.iter() {
                // Only show dotfiles belonging to the current repo
                if dotfile.source_path.starts_with(&target) {
                    if dotfile.is_modified(&db) {
                        println!(
                            "    {} -> {}",
                            target_path.to_string_lossy().bold(),
                            "modified".yellow()
                        );
                    } else if dotfile.is_outdated() {
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
