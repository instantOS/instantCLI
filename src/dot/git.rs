use anyhow::{Context, Result};
use std::{process::Command, path::PathBuf};
use crate::dot::config;
use crate::dot::localrepo as repo_mod;

pub fn add_repo(repo: config::Repo, debug: bool) -> Result<PathBuf> {
    let base = config::repos_base_dir()?;

    let repo_dir_name = match &repo.name {
        Some(n) => n.clone(),
        None => basename_from_repo(&repo.url),
    };

    let target = base.join(repo_dir_name);

    if target.exists() {
        return Err(anyhow::anyhow!("Destination '{}' already exists", target.display()));
    }

    let mut cmd = Command::new("git");
    cmd.arg("clone");
    cmd.arg("--depth").arg("1");
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
    let mut repos = config::load_repos()?;
    repos.push(repo);
    config::save_repos(&repos)?;

    Ok(target)
}

pub fn update_all(debug: bool) -> Result<()> {
    let repos = config::load_repos()?;
    let base = config::repos_base_dir()?;
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    let mut any_failed = false;

    for crepo in repos.iter() {
        let repo: repo_mod::LocalRepo = crepo.clone().into();
        if let Err(e) = repo.update(debug) {
            eprintln!("Failed to update {}: {}", crepo.url, e);
            any_failed = true;
        }
    }

    if any_failed {
        Err(anyhow::anyhow!("One or more repos failed to update"))
    } else {
        Ok(())
    }
}

pub fn status_all(debug: bool) -> Result<()> {
    let repos = config::load_repos()?;
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
            println!("{} -> missing at {}", crepo.url, target.display());
            continue;
        }

        // validate instantdots.toml exists and parse it via LocalRepo
        let local: repo_mod::LocalRepo = crepo.clone().into();
        match local.read_meta() {
            Ok(meta) => {
                if debug {
                    eprintln!("Repo {} identified as dot repo '{}' - {}", crepo.url, meta.name, meta.description.as_deref().unwrap_or(""));
                }
            }
            Err(e) => {
                println!("{} -> not a valid instantdots repo: {}", crepo.url, e);
                continue;
            }
        }

        let branch = match &crepo.branch {
            Some(b) => b.clone(),
            None => "(no branch configured)".to_string(),
        };

        // get checked out branch
        let current_branch = match local.get_branch() {
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
            println!("{} -> clean (branch: {}, configured: {})", crepo.url, current_branch, branch);
        } else {
            println!("{} -> modified (branch: {}, configured: {})\n{}", crepo.url, current_branch, branch, stdout);
        }
    }

    Ok(())
}

fn basename_from_repo(repo: &str) -> String {
    let s = repo.trim_end_matches(".git");
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
