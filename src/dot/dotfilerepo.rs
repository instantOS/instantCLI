use crate::common;
use crate::common::git;
use crate::dot::config::Config;
use anyhow::{Context, Result};
use colored::Colorize;
use git2::Repository;
use std::{path::Path, path::PathBuf};

/// Represents a single dotfile directory within a repository
#[derive(Debug, Clone)]
pub struct DotfileDir {
    pub path: PathBuf,
    pub is_active: bool,
}

impl DotfileDir {
    pub fn new(name: &str, repo_path: &Path, is_active: bool) -> Result<Self> {
        let path = repo_path.join(name);

        // Create directory if it doesn't exist (git doesn't track empty directories)
        if !path.exists() {
            std::fs::create_dir_all(&path).with_context(|| {
                format!("Failed to create dotfile directory '{}'", path.display())
            })?;

            // Notify user about created directory
            crate::ui::emit(
                crate::ui::Level::Success,
                "dot.repo.directory_created",
                &format!(
                    "{} Created directory: {}",
                    char::from(crate::ui::nerd_font::NerdFont::Folder),
                    path.display().to_string().cyan()
                ),
                None,
            );
        }

        Ok(DotfileDir { path, is_active })
    }

    /// Create a DotfileDir without creating the directory if it doesn't exist.
    /// Used for discovery where we only want to include directories that exist.
    pub fn new_no_create(name: &str, repo_path: &Path, is_active: bool) -> Result<Self> {
        let path = repo_path.join(name);
        Ok(DotfileDir { path, is_active })
    }
}

#[derive(Clone, Debug)]
pub struct DotfileRepo {
    pub url: String,
    pub name: String,
    pub branch: Option<String>,
    pub dotfile_dirs: Vec<DotfileDir>,
    pub meta: crate::dot::types::RepoMetaData,
}

impl DotfileRepo {
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

        // Read metadata file or use config metadata
        let meta = if let Some(meta) = &repo_config.metadata {
            meta.clone()
        } else {
            crate::dot::meta::read_meta(&local_path)
                .with_context(|| format!("Failed to read metadata for repository '{name}'"))?
        };

        // Note: We allow metadata name to differ from config name to support flexible naming

        // Get active subdirectories from config (used to set the 'active' field on dotfile directories)
        let active_subdirs = cfg.get_active_subdirs(&name);

        // Create dotfile_dirs
        let dotfile_dirs =
            Self::dotfile_dirs_from_path(&local_path, &meta.dots_dirs, &active_subdirs)?;

        Ok(DotfileRepo {
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
    /// - active_subdirs: Effective active subdirectories (from config + defaults)
    ///
    /// This creates DotfileDir instances for ALL available subdirectories.
    /// The ORDER of active_subdirs determines priority - earlier entries have higher priority.
    /// This provides a unified priority system where "higher in the list = higher priority".
    fn dotfile_dirs_from_path(
        repo_path: &Path,
        available_subdirs: &[String],
        active_subdirs: &[String],
    ) -> Result<Vec<DotfileDir>> {
        let mut dotfile_dirs = Vec::new();
        let mut added_subdirs = std::collections::HashSet::new();

        // Phase 1: Active subdirs in priority order
        // Include if in metadata OR if the directory exists on disk
        for subdir_name in active_subdirs {
            let in_metadata = available_subdirs.contains(subdir_name);
            let subdir_path = repo_path.join(subdir_name);
            let exists_on_disk = subdir_path.exists();

            if in_metadata || exists_on_disk {
                let is_active = true;
                let dotfile_dir = DotfileDir::new_no_create(subdir_name, repo_path, is_active)?;
                dotfile_dirs.push(dotfile_dir);
                added_subdirs.insert(subdir_name.clone());
            }
        }

        // Phase 2: Inactive subdirs from metadata (not active, but tracked)
        for subdir_name in available_subdirs {
            if !added_subdirs.contains(subdir_name) {
                let dotfile_dir = DotfileDir::new_no_create(subdir_name, repo_path, false)?;
                dotfile_dirs.push(dotfile_dir);
            }
        }

        Ok(dotfile_dirs)
    }

    pub fn active_dotfile_dirs(&self) -> impl Iterator<Item = &DotfileDir> {
        self.dotfile_dirs.iter().filter(|dir| dir.is_active)
    }

    pub fn get_checked_out_branch(&self, cfg: &Config) -> Result<String> {
        let target = self.local_path(cfg)?;
        let repo = Repository::open(&target).context("Failed to open git repository")?;
        git::current_branch(&repo).context("Failed to get current branch")
    }

    /// Get subdirs that are enabled in config but not in metadata's dots_dirs.
    /// These are "orphaned" subdirs that may indicate a configuration mismatch.
    pub fn get_orphaned_active_subdirs(&self, config: &Config) -> Vec<String> {
        let active_subdirs = config.get_active_subdirs(&self.name);
        active_subdirs
            .into_iter()
            .filter(|subdir| !self.meta.dots_dirs.contains(subdir))
            .collect()
    }

    /// Check if this is an external repo (metadata from config, not instantdots.toml)
    pub fn is_external(&self, config: &Config) -> bool {
        config
            .repos
            .iter()
            .find(|r| r.name == self.name)
            .map(|r| r.metadata.is_some())
            .unwrap_or(false)
    }

    /// Convert a target path (in home directory) to source path (in repo)

    #[allow(dead_code)]
    pub fn target_to_source(&self, target_path: &Path) -> Result<Option<PathBuf>> {
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
            let pb = common::progress::create_spinner(format!("Fetching branch {branch}..."));

            let mut repo =
                Repository::open(&target).context("Failed to open git repository for fetch")?;
            git::fetch_branch(&mut repo, branch).context("Failed to fetch branch")?;

            common::progress::finish_spinner_with_success(pb, format!("Fetched branch {branch}"));

            let pb = common::progress::create_spinner(format!("Checking out {branch}..."));

            git::checkout_branch(&mut repo, branch).context("Failed to checkout branch")?;

            common::progress::finish_spinner_with_success(pb, format!("Checked out {branch}"));
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
        let pb = common::progress::create_spinner(format!("Updating {}...", self.name));

        let mut repo =
            Repository::open(&target).context("Failed to open git repository for pull")?;
        git::clean_and_pull(&mut repo).context("Failed to pull latest changes")?;

        common::progress::finish_spinner_with_success(pb, format!("Updated {}", self.name));

        Ok(())
    }
}
