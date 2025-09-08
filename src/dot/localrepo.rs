use crate::dot::config::{self, Config};
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
    pub fn new(name: String, repo_path: &PathBuf, is_active: bool) -> Result<Self> {
        let path = repo_path.join(&name);
        
        // Check if path exists on creation
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Dotfile directory '{}' does not exist at '{}'",
                name,
                path.display()
            ));
        }
        
        Ok(DotfileDir {
            name,
            path,
            is_active,
        })
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

}

#[derive(Clone, Debug)]
pub struct LocalRepo {
    pub url: String,
    pub name: String,
    pub branch: Option<String>,
    pub dotfile_dirs: Vec<DotfileDir>,
    pub meta: crate::dot::meta::RepoMetaData,
}

impl LocalRepo {
    pub fn new(cfg: &Config, name: String) -> Result<Self> {
        // Check if the name exists in the config
        let repo_config = cfg
            .repos
            .iter()
            .find(|repo| repo.name == name)
            .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found in configuration", name))?;

        // Get the local path where the repo should be
        let local_path = Self::local_path_from_name(&name)?;

        // Check if the repo directory exists
        if !local_path.exists() {
            return Err(anyhow::anyhow!(
                "Repository directory '{}' does not exist",
                local_path.display()
            ));
        }

        // Read metadata file
        let meta = crate::dot::meta::read_meta(&local_path)
            .with_context(|| format!("Failed to read metadata for repository '{}'", name))?;

        // Validate that metadata name matches config name
        if meta.name != name {
            return Err(anyhow::anyhow!(
                "Metadata name '{}' does not match config name '{}' for repository",
                meta.name,
                name
            ));
        }

        // Get active subdirectories from config (used to set the 'active' field on dotfile directories)
        let active_subdirs = cfg
            .get_active_subdirs(&name)
            .unwrap_or_else(|| vec!["dots".to_string()]);

        // Create dotfile_dirs
        let dotfile_dirs =
            Self::dotfile_dirs_from_path(&local_path, &meta.dots_dirs, &active_subdirs)?;

        Ok(LocalRepo {
            url: repo_config.url.clone(),
            name,
            branch: repo_config.branch.clone(),
            dotfile_dirs,
            meta,
        })
    }

    pub fn local_path(&self) -> Result<PathBuf> {
        let base = config::repos_dir()?;
        Ok(base.join(&self.name))
    }

    fn local_path_from_name(name: &str) -> Result<PathBuf> {
        let base = config::repos_dir()?;
        Ok(base.join(name))
    }

    /// Create DotfileDir instances for all available subdirectories from metadata
    /// 
    /// Arguments:
    /// - available_subdirs: All subdirectories configured in the repo metadata
    /// - active_subdirs: Subdirectories that should be marked as active (from config)
    /// 
    /// This creates DotfileDir instances for ALL available subdirectories,
    /// using active_subdirs only to determine which ones should be active.
    fn dotfile_dirs_from_path(
        repo_path: &PathBuf,
        available_subdirs: &[String],
        active_subdirs: &[String],
    ) -> Result<Vec<DotfileDir>> {
        let mut dotfile_dirs = Vec::new();

        for subdir_name in available_subdirs {
            let is_active = active_subdirs.contains(subdir_name);
            let dotfile_dir = DotfileDir::new(subdir_name.clone(), repo_path, is_active)?;
            dotfile_dirs.push(dotfile_dir);
        }

        Ok(dotfile_dirs)
    }

    pub fn get_checked_out_branch(&self) -> Result<String> {
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

    /// Get all dotfiles from this repository for active subdirectories
    pub fn get_all_dotfiles(
        &self,
    ) -> Result<HashMap<PathBuf, crate::dot::dotfile::Dotfile>> {
        let mut filemap = HashMap::new();

        // Get dotfiles from active directories
        for dotfile_dir in self.dotfile_dirs.iter() {
            if dotfile_dir.is_active {
                match dotfile_dir.get_dotfiles() {
                    Ok(dotfiles) => {
                        for dotfile in dotfiles {
                            // dotfile dirs override each other
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
        _config: &crate::dot::config::Config,
    ) -> Result<Option<PathBuf>> {
        let home = std::path::PathBuf::from(shellexpand::tilde("~").to_string());
        let relative = target_path.strip_prefix(&home).unwrap_or(target_path);

        // Try to find the source path in active dotfile directories
        for dotfile_dir in &self.dotfile_dirs {
            if dotfile_dir.is_active {
                let source_path = dotfile_dir.path.join(relative);
                if source_path.exists() {
                    return Ok(Some(source_path));
                }
            }
        }

        Ok(None)
    }

    fn switch_branch(&self, branch: &str, debug: bool) -> Result<()> {
        let target = self.local_path()?;
        let current = self.get_checked_out_branch()?;
        if current != branch {
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
                .with_context(|| format!("fetching branch {} in {}", branch, target.display()))?;

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
        Ok(())
    }

    pub fn update(&self, debug: bool) -> Result<()> {
        // Note: Repo existence is verified in LocalRepo::new(), no need to check again
        let target = self.local_path()?;

        // If branch is specified, ensure we're on that branch
        if let Some(branch) = &self.branch {
            self.switch_branch(branch, debug)?;
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
