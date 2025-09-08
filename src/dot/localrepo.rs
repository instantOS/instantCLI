use crate::dot::config;
use crate::dot::utils;
use anyhow::{Context, Result};
use std::{collections::HashMap, path::Path, path::PathBuf, process::Command};
use walkdir::WalkDir;

/// Represents a single dotfile directory within a repository
#[derive(Debug, Clone)]
pub struct DotfileDir {
    pub name: String,
    pub path: PathBuf,
    pub is_active: bool,
}

impl DotfileDir {
    pub fn new(name: String, repo_path: &PathBuf, is_active: bool) -> Self {
        let path = repo_path.join(&name);
        DotfileDir {
            name,
            path,
            is_active,
        }
    }

    /// Check if this dotfile directory exists on disk
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Get all dotfiles in this directory
    pub fn get_dotfiles(&self) -> Result<Vec<(PathBuf, PathBuf)>> {
        let mut dotfiles = Vec::new();
        let home = PathBuf::from(shellexpand::tilde("~").to_string());

        if !self.exists() {
            return Ok(dotfiles);
        }

        for entry in WalkDir::new(&self.path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| {
                let path_str = entry.path().to_string_lossy();
                !path_str.contains("/.git/")
            })
        {
            if entry.file_type().is_file() {
                let source_path = entry.path().to_path_buf();
                let relative_path = source_path.strip_prefix(&self.path).unwrap().to_path_buf();
                let target_path = home.join(relative_path);
                dotfiles.push((source_path, target_path));
            }
        }

        Ok(dotfiles)
    }

    /// Convert a target path to source path within this directory
    pub fn target_to_source(&self, target_path: &Path) -> Option<PathBuf> {
        let home = std::path::PathBuf::from(shellexpand::tilde("~").to_string());
        let relative = target_path.strip_prefix(&home).unwrap_or(target_path);
        let source_path = self.path.join(relative);
        if source_path.exists() {
            Some(source_path)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct LocalRepo {
    pub url: String,
    pub name: String, // Now mandatory
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

    /// Create DotfileDir instances for this repository
    pub fn create_dotfile_dirs(
        &self,
        available_subdirs: &[String],
        active_subdirs: &[String],
    ) -> Vec<DotfileDir> {
        let repo_path = self.local_path().unwrap_or_else(|_| PathBuf::new());
        let mut dotfile_dirs = Vec::new();

        for subdir_name in available_subdirs {
            let is_active = active_subdirs.contains(subdir_name);
            let dotfile_dir = DotfileDir::new(subdir_name.clone(), &repo_path, is_active);
            dotfile_dirs.push(dotfile_dir);
        }

        dotfile_dirs
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
        let config = crate::dot::config::Config::load()?;
        let active_subdirs = config
            .get_active_subdirs(&self.name)
            .unwrap_or_else(|| vec!["dots".to_string()]);

        // Create DotfileDir instances
        let dotfile_dirs = self.create_dotfile_dirs(&meta.dots_dirs, &active_subdirs);

        // Return paths for active directories that exist
        let mut active_dirs = Vec::new();
        for dotfile_dir in dotfile_dirs {
            if dotfile_dir.is_active && dotfile_dir.exists() {
                active_dirs.push(dotfile_dir.path);
            }
        }

        Ok(active_dirs)
    }

    /// Get all dotfiles from this repository for active subdirectories
    pub fn get_all_dotfiles(&self) -> Result<HashMap<PathBuf, crate::dot::dotfile::Dotfile>> {
        let mut filemap = HashMap::new();

        // Get DotfileDir instances for this repo
        let meta = self.read_meta()?;
        let config = crate::dot::config::Config::load()?;
        let active_subdirs = config
            .get_active_subdirs(&self.name)
            .unwrap_or_else(|| vec!["dots".to_string()]);

        let dotfile_dirs = self.create_dotfile_dirs(&meta.dots_dirs, &active_subdirs);

        // Get dotfiles from active directories
        for dotfile_dir in dotfile_dirs {
            if dotfile_dir.is_active && dotfile_dir.exists() {
                match dotfile_dir.get_dotfiles() {
                    Ok(dotfiles) => {
                        for (source_path, target_path) in dotfiles {
                            let dotfile = crate::dot::dotfile::Dotfile {
                                source_path: source_path,
                                target_path: target_path.clone(),
                                hash: None,
                                target_hash: None,
                            };
                            filemap.insert(target_path, dotfile);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to get dotfiles from {}: {}",
                            dotfile_dir.name, e
                        );
                    }
                }
            }
        }

        Ok(filemap)
    }

    /// Convert a target path (in home directory) to source path (in repo)
    pub fn target_to_source(&self, target_path: &Path) -> Result<Option<PathBuf>> {
        // Get DotfileDir instances for this repo
        let meta = self.read_meta()?;
        let config = crate::dot::config::Config::load()?;
        let active_subdirs = config
            .get_active_subdirs(&self.name)
            .unwrap_or_else(|| vec!["dots".to_string()]);

        let dotfile_dirs = self.create_dotfile_dirs(&meta.dots_dirs, &active_subdirs);

        // Try to find the source path in active directories
        for dotfile_dir in dotfile_dirs {
            if dotfile_dir.is_active && dotfile_dir.exists() {
                if let Some(source_path) = dotfile_dir.target_to_source(target_path) {
                    return Ok(Some(source_path));
                }
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
                let pb = utils::create_spinner(format!("Fetching branch {}...", branch));

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

                pb.finish_with_message(format!("Fetched branch {}", branch));

                if !fetch.status.success() {
                    let stderr = String::from_utf8_lossy(&fetch.stderr);
                    return Err(anyhow::anyhow!("git fetch failed: {}", stderr));
                }

                let pb = utils::create_spinner(format!("Checking out {}...", branch));

                let co = Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .arg("checkout")
                    .arg(branch)
                    .output()
                    .with_context(|| {
                        format!("checking out branch {} in {}", branch, target.display())
                    })?;

                pb.finish_with_message(format!("Checked out {}", branch));

                if !co.status.success() {
                    let stderr = String::from_utf8_lossy(&co.stderr);
                    return Err(anyhow::anyhow!("git checkout failed: {}", stderr));
                }
            }
        }

        // pull latest
        let pb = utils::create_spinner(format!("Updating {}...", self.name));

        let pull = Command::new("git")
            .arg("-C")
            .arg(&target)
            .arg("pull")
            .output()
            .with_context(|| format!("running git pull in {}", target.display()))?;

        pb.finish_with_message(format!("Updated {}", self.name));

        if !pull.status.success() {
            let stderr = String::from_utf8_lossy(&pull.stderr);
            return Err(anyhow::anyhow!("git pull failed: {}", stderr));
        }

        Ok(())
    }
}
