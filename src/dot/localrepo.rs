use crate::dot::config::Config;
use crate::dot::utils;
use anyhow::{Context, Result};
use std::{path::Path, path::PathBuf};

/// Represents a single dotfile directory within a repository
#[derive(Debug, Clone)]
pub struct DotfileDir {
    pub path: PathBuf,
    pub is_active: bool,
}

impl DotfileDir {
    pub fn new(name: &str, repo_path: &PathBuf, is_active: bool) -> Result<Self> {
        let path = repo_path.join(name);

        // Check if path exists on creation
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Dotfile directory '{}' does not exist at '{}'",
                name,
                path.display()
            ));
        }

        Ok(DotfileDir { path, is_active })
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
        let local_path = Self::local_path_from_name(cfg, &name)?;

        // Check if the repo directory exists
        if !local_path.exists() {
            return Err(anyhow::anyhow!(
                "Repository directory '{}' does not exist",
                local_path.display()
            ));
        }

        // Read metadata file
        let meta = crate::dot::meta::read_meta(&local_path)
            .with_context(|| format!("Failed to read metadata for repository '{name}'"))?;

        // Note: We allow metadata name to differ from config name to support flexible naming

        // Get active subdirectories from config (used to set the 'active' field on dotfile directories)
        let active_subdirs = cfg.get_active_subdirs(&name);

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

    pub fn local_path(&self, cfg: &Config) -> Result<PathBuf> {
        Ok(cfg.repos_path().join(&self.name))
    }

    fn local_path_from_name(cfg: &Config, name: &str) -> Result<PathBuf> {
        Ok(cfg.repos_path().join(name))
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
            let dotfile_dir = DotfileDir::new(subdir_name, repo_path, is_active)?;
            dotfile_dirs.push(dotfile_dir);
        }

        Ok(dotfile_dirs)
    }

    pub fn get_checked_out_branch(&self, cfg: &Config) -> Result<String> {
        let target = self.local_path(cfg)?;
        utils::git_command_in_dir(
            &target,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            "determining current branch",
        )
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

    fn switch_branch(&self, cfg: &Config, branch: &str, debug: bool) -> Result<()> {
        let target = self.local_path(cfg)?;
        let current = self.get_checked_out_branch(cfg)?;
        if current != branch {
            if debug {
                eprintln!("Switching {current} -> {branch}");
            } else {
                println!("Switching {current} -> {branch}");
            }

            // fetch the branch and checkout
            let pb = utils::create_spinner(format!("Fetching branch {branch}..."));

            utils::git_command_in_dir_with_output(
                &target,
                &["fetch", "origin", branch],
                &format!("fetching branch {} in {}", branch, target.display()),
            )?;

            pb.finish_with_message(format!("Fetched branch {branch}"));

            let pb = utils::create_spinner(format!("Checking out {branch}..."));

            utils::git_command_in_dir_with_output(
                &target,
                &["checkout", branch],
                &format!("checking out branch {} in {}", branch, target.display()),
            )?;

            pb.finish_with_message(format!("Checked out {branch}"));
        }
        Ok(())
    }

    pub fn update(&self, cfg: &Config, debug: bool) -> Result<()> {
        let target = self.local_path(cfg)?;

        // If branch is specified, ensure we're on that branch
        if let Some(branch) = &self.branch {
            self.switch_branch(cfg, branch, debug)?;
        }

        // pull latest
        let pb = utils::create_spinner(format!("Updating {}...", self.name));

        utils::git_command_in_dir_with_output(
            &target,
            &["pull"],
            &format!("running git pull in {}", target.display()),
        )?;

        pb.finish_with_message(format!("Updated {}", self.name));

        Ok(())
    }
}
