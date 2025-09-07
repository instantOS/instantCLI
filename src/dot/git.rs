use crate::dot::config;
use crate::dot::config::basename_from_repo;
use crate::dot::localrepo as repo_mod;
use anyhow::{Context, Result};
use colored::*;
use std::{path::PathBuf, process::Command};

pub fn add_repo(repo: config::Repo, debug: bool) -> Result<PathBuf> {
    let base = config::repos_base_dir()?;

    let repo_dir_name = match &repo.name {
        Some(n) => n.clone(),
        None => basename_from_repo(&repo.url),
    };

    let target = base.join(repo_dir_name);

    if target.exists() {
        return Err(anyhow::anyhow!(
            "Destination '{}' already exists",
            target.display()
        ));
    }

    let mut cfg = config::Config::load()?;
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

    let output = cmd.output().context("running git clone")?;
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

pub fn update_all(debug: bool) -> Result<()> {
    let cfg = config::Config::load()?;
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

pub fn status_all(debug: bool) -> Result<()> {
    let cfg = config::Config::load()?;
    let repos = cfg.repos.clone();
    let base = config::repos_base_dir()?;
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    for crepo in repos.iter() {
        let repo_dir_name = match &crepo.name {
            Some(n) => n.clone(),
            None => basename_from_repo(&crepo.url),
        };
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
        let filemap = super::get_all_dotfiles()?;

        for (target_path, dotfile) in filemap.iter() {
            // Only show dotfiles belonging to the current repo
            if dotfile.repo_path.starts_with(&target) {
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

    Ok(())
}
