use anyhow::{Context, Result};
use std::{path::PathBuf, process::Command};
use crate::dot::config;

#[derive(Clone, Debug)]
pub struct Repo {
    pub url: String,
    pub name: Option<String>,
    pub branch: Option<String>,
}

impl From<config::Repo> for Repo {
    fn from(r: config::Repo) -> Self {
        Repo { url: r.url, name: r.name, branch: r.branch }
    }
}

impl From<Repo> for config::Repo {
    fn from(r: Repo) -> Self {
        config::Repo { url: r.url, name: r.name, branch: r.branch }
    }
}

impl Repo {
    pub fn local_path(&self) -> Result<PathBuf> {
        let base = config::repos_base_dir()?;
        let repo_dir_name = match &self.name {
            Some(n) => n.clone(),
            None => basename_from_repo(&self.url),
        };
        Ok(base.join(repo_dir_name))
    }

    pub fn update(&self, debug: bool) -> Result<()> {
        let target = self.local_path()?;
        if !target.exists() {
            return Err(anyhow::anyhow!("Repo destination '{}' does not exist", target.display()));
        }

        // If branch is specified, ensure we're on that branch
        if let Some(branch) = &self.branch {
            let out = Command::new("git")
                .arg("-C")
                .arg(&target)
                .arg("rev-parse")
                .arg("--abbrev-ref")
                .arg("HEAD")
                .output()
                .context("determining current branch")?;

            let current = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if current != *branch {
                if debug {
                    eprintln!("Switching {} -> {}", current, branch);
                } else {
                    println!("Switching {} -> {}", current, branch);
                }

                // fetch the branch and checkout
                let fetch = Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .arg("fetch")
                    .arg("origin")
                    .arg(branch)
                    .output()
                    .with_context(|| format!("fetching branch {} in {}", branch, target.display()))?;

                if !fetch.status.success() {
                    let stderr = String::from_utf8_lossy(&fetch.stderr);
                    return Err(anyhow::anyhow!("git fetch failed: {}", stderr));
                }

                let co = Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .arg("checkout")
                    .arg(branch)
                    .output()
                    .with_context(|| format!("checking out branch {} in {}", branch, target.display()))?;

                if !co.status.success() {
                    let stderr = String::from_utf8_lossy(&co.stderr);
                    return Err(anyhow::anyhow!("git checkout failed: {}", stderr));
                }
            }
        }

        // pull latest
        let pull = Command::new("git")
            .arg("-C")
            .arg(&target)
            .arg("pull")
            .output()
            .with_context(|| format!("running git pull in {}", target.display()))?;

        if !pull.status.success() {
            let stderr = String::from_utf8_lossy(&pull.stderr);
            return Err(anyhow::anyhow!("git pull failed: {}", stderr));
        }

        Ok(())
    }
}

fn basename_from_repo(repo: &str) -> String {
    let s = repo.trim_end_matches(".git");
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
