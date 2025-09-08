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

    /// Get all dotfiles in this directory
    pub fn get_dotfiles(&self) -> Result<Vec<crate::dot::dotfile::Dotfile>> {
        let mut dotfiles = Vec::new();

        if !self.path.exists() {
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
                let target_path =
                    PathBuf::from(shellexpand::tilde("~").to_string()).join(relative_path);

                let dotfile = crate::dot::dotfile::Dotfile {
                    source_path,
                    target_path: target_path.clone(),
                    hash: None,
                    target_hash: None,
                };
                dotfiles.push(dotfile);
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
    pub name: String,
    pub branch: Option<String>,
    pub dotfile_dirs: Vec<DotfileDir>,
}

impl From<config::Repo> for LocalRepo {
    fn from(r: config::Repo) -> Self {
        LocalRepo {
            url: r.url,
            name: r.name,
            branch: r.branch,
            dotfile_dirs: Vec::new(), // Initialize empty, will be populated when needed
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

    /// Get active dotfile directory information for this repo
    pub fn get_active_dotfile_dirs(
        &self,
        config: &crate::dot::config::Config,
    ) -> Result<Vec<crate::dot::DotfileDirInfo>> {
        let mut active_dirs = Vec::new();
        let repo_path = self.local_path()?;

        // Read repo metadata to get available dots directories
        // TODO: a localrepo should read its meta upon creation and fail to create if the metadata is invalid
        // replace any self.read_metadata calls
        let meta = self.read_meta()?;
        let active_subdirs = config
            .get_active_subdirs(&self.name)
            .unwrap_or_else(|| vec!["dots".to_string()]);

        // Process active subdirectories in the order they appear in active_subdirs
        for subdir_name in active_subdirs {
            // Check if this subdirectory exists in the repo's metadata
            if !meta.dots_dirs.contains(&subdir_name) {
                continue;
            }

            let dir_path = repo_path.join(&subdir_name);
            if dir_path.exists() {
                active_dirs.push(crate::dot::DotfileDirInfo {
                    repo_name: self.name.clone(),
                    repo_path: repo_path.clone(),
                    subdir_name: subdir_name.clone(),
                    dir_path: dir_path.clone(),
                    is_active: true,
                });
            }
        }

        Ok(active_dirs)
    }

    /// Get all dotfiles from this repository for active subdirectories
    pub fn get_all_dotfiles(
        &self,
        config: &crate::dot::config::Config,
    ) -> Result<HashMap<PathBuf, crate::dot::dotfile::Dotfile>> {
        let mut filemap = HashMap::new();

        // Get DotfileDir instances for this repo
        let meta = self.read_meta()?;
        let active_subdirs = config
            .get_active_subdirs(&self.name)
            .unwrap_or_else(|| vec!["dots".to_string()]);

        let dotfile_dirs = self.create_dotfile_dirs(&meta.dots_dirs, &active_subdirs);

        // Get dotfiles from active directories
        for dotfile_dir in dotfile_dirs {
            if dotfile_dir.is_active && dotfile_dir.path.exists() {
                match dotfile_dir.get_dotfiles() {
                    Ok(dotfiles) => {
                        for dotfile in dotfiles {
                            filemap.insert(dotfile.target_path.clone(), dotfile);
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
    pub fn target_to_source(
        &self,
        target_path: &Path,
        config: &crate::dot::config::Config,
    ) -> Result<Option<PathBuf>> {
        let active_dirs = crate::dot::get_active_dotfile_dirs(config)?;
        let repo_active_dirs: Vec<_> = active_dirs
            .into_iter()
            .filter(|dir| dir.repo_name == self.name)
            .collect();

        // Try to find the source path in active directories
        for dotfile_dir in repo_active_dirs {
            let home = std::path::PathBuf::from(shellexpand::tilde("~").to_string());
            let relative = target_path.strip_prefix(&home).unwrap_or(target_path);
            let source_path = dotfile_dir.dir_path.join(relative);
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
        // TODO: extract this into a separate function
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
