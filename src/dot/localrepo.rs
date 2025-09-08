use crate::dot::config;
use anyhow::{Context, Result};
use std::{path::Path, path::PathBuf, process::Command};

#[derive(Clone, Debug)]
pub struct LocalRepo {
    pub url: String,
    pub name: String,  // Now mandatory
    pub branch: Option<String>,
}

impl From<config::Repo> for LocalRepo {
    fn from(r: config::Repo) -> Self {
        LocalRepo {
            url: r.url,
            name: r.name,
            branch: r.branch,
        }
    }
}

impl From<LocalRepo> for config::Repo {
    fn from(r: LocalRepo) -> Self {
        config::Repo {
            url: r.url,
            name: r.name,
            branch: r.branch,
            active_subdirs: Vec::new(), // Default empty, will be set by config
        }
    }
}

impl LocalRepo {
    pub fn local_path(&self) -> Result<PathBuf> {
        let base = config::repos_base_dir()?;
        Ok(base.join(&self.name))
    }

    pub fn get_branch(&self) -> Result<String> {
        let target = self.local_path()?;
        let out = Command::new("git")
            .arg("-C")
            .arg(&target)
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD")
            .output()
            .context("determining current branch")?;
        let current = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(current)
    }

    pub fn read_meta(&self) -> Result<crate::dot::meta::RepoMetaData> {
        let target = self.local_path()?;
        crate::dot::meta::read_meta(&target)
    }

    /// Get all active dots directories for this repo
    pub fn get_active_dots_dirs(&self) -> Result<Vec<PathBuf>> {
        let meta = self.read_meta()?;
        // Config is now cached to avoid loading multiple times per app run
        let config = crate::dot::config::Config::load()?;
        let active_subdirs = config.get_active_subdirs(&self.name)
            .unwrap_or_else(|| vec!["dots".to_string()]);
        
        let repo_path = self.local_path()?;
        let mut active_dirs = Vec::new();
        
        for subdir in active_subdirs {
            if meta.dots_dirs.contains(&subdir) {
                let dir_path = repo_path.join(&subdir);
                if dir_path.exists() {
                    active_dirs.push(dir_path);
                }
            }
        }
        
        Ok(active_dirs)
    }

    /// Find which dots directory a source file belongs to
    pub fn find_dots_dir_for_file(&self, source_file: &Path) -> Result<Option<PathBuf>> {
        let active_dirs = self.get_active_dots_dirs()?;
        
        for dots_dir in active_dirs {
            if source_file.starts_with(&dots_dir) {
                return Ok(Some(dots_dir));
            }
        }
        
        Ok(None)
    }

    /// Convert a target path (in home directory) to source path (in repo)
    pub fn target_to_source(&self, target_path: &Path) -> Result<Option<PathBuf>> {
        let home = std::path::PathBuf::from(shellexpand::tilde("~").to_string());
        let relative = target_path.strip_prefix(&home).unwrap_or(target_path);
        
        let active_dirs = self.get_active_dots_dirs()?;
        
        for dots_dir in active_dirs {
            let source_path = dots_dir.join(relative);
            if source_path.exists() {
                return Ok(Some(source_path));
            }
        }
        
        Ok(None)
    }

    pub fn update(&self, debug: bool) -> Result<()> {
        let target = self.local_path()?;
        if !target.exists() {
            return Err(anyhow::anyhow!(
                "Repo destination '{}' does not exist",
                target.display()
            ));
        }

        // If branch is specified, ensure we're on that branch
        if let Some(branch) = &self.branch {
            let current = self.get_branch()?;
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
                    .with_context(|| {
                        format!("fetching branch {} in {}", branch, target.display())
                    })?;

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
                    .with_context(|| {
                        format!("checking out branch {} in {}", branch, target.display())
                    })?;

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
