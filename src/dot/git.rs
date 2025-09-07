use anyhow::{Context, Result};
use std::{process::Command, path::PathBuf};
use crate::dot::config;

pub fn add_repo(repo: config::Repo, debug: bool) -> Result<PathBuf> {
    let base = config::repos_base_dir_path()?;

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
    let base = config::repos_base_dir_path()?;
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    let mut any_failed = false;

    for repo in repos.iter() {
        let repo_dir_name = match &repo.name {
            Some(n) => n.clone(),
            None => basename_from_repo(&repo.url),
        };
        let target = base.join(repo_dir_name);

        if !target.exists() {
            eprintln!("Repo destination '{}' does not exist, skipping", target.display());
            any_failed = true;
            continue;
        }

        if debug {
            eprintln!("Updating repo at {}", target.display());
        } else {
            println!("Updating {}", target.display());
        }

        let output = Command::new("git")
            .arg("-C")
            .arg(&target)
            .arg("pull")
            .output()
            .with_context(|| format!("running git pull in {}", target.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("Failed to update {}: {}", target.display(), stderr);
            any_failed = true;
        } else if debug {
            let stdout = String::from_utf8_lossy(&output.stdout);
            eprintln!("Updated {}: {}", target.display(), stdout);
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
    let base = config::repos_base_dir_path()?;
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    for repo in repos.iter() {
        let repo_dir_name = match &repo.name {
            Some(n) => n.clone(),
            None => basename_from_repo(&repo.url),
        };
        let target = base.join(repo_dir_name);

        if !target.exists() {
            println!("{} -> missing at {}", repo.url, target.display());
            continue;
        }

        let output = Command::new("git")
            .arg("-C")
            .arg(&target)
            .arg("status")
            .arg("--porcelain")
            .output()
            .with_context(|| format!("running git status in {}", target.display()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            println!("{} -> clean", repo.url);
        } else {
            println!("{} -> modified\n{}", repo.url, stdout);
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
