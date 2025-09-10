use anyhow::Result;
use colored::Colorize;
use crate::dot::config::{Config, Repo};
use crate::dot::localrepo::LocalRepo;
use crate::dot::db::Database;

/// RepositoryManager provides centralized iteration and management of repositories
/// following the existing borrowed references pattern
pub struct RepositoryManager<'a> {
    //TODO: should this be a mutable reference?
    config: &'a Config,
    db: &'a Database,
}

impl<'a> RepositoryManager<'a> {
    /// Create a new RepositoryManager with borrowed references
    pub fn new(config: &'a Config, db: &'a Database) -> Self {
        Self { config, db }
    }

    /// Add a new repository to the configuration
    pub fn add_repository(&self, url: &str, name: Option<String>, branch: Option<String>) -> Result<()> {
        let repo_name = name.unwrap_or_else(|| extract_repo_name(url));
        let repo = Repo {
            url: url.to_string(),
            name: repo_name.clone(),
            branch,
            active_subdirectories: vec!["dots".to_string()], // Default
            enabled: true,
        };

        // This would need to be done through a mutable config reference
        // For now, we'll create a helper that can be used by the actual implementation
        Err(anyhow::anyhow!("Use Config::add_repo directly for adding repositories"))
    }

    /// Remove a repository from the configuration
    pub fn remove_repository(&self, name: &str, remove_files: bool) -> Result<()> {
        // Implementation would go here
        Err(anyhow::anyhow!("Use Config::remove_repo directly for removing repositories"))
    }

    /// Enable a repository
    pub fn enable_repository(&self, name: &str) -> Result<()> {
        // This would need mutable config access
        // TODO: should this even exist if config already has this logic?
        Err(anyhow::anyhow!("Use Config::enable_repo directly for enabling repositories"))
    }

    /// Disable a repository
    pub fn disable_repository(&self, name: &str) -> Result<()> {
        // This would need mutable config access
        // TODO: should this even exist if config already has this logic?
        Err(anyhow::anyhow!("Use Config::disable_repo directly for disabling repositories"))
    }

    /// Execute a callback for each enabled repository
    pub fn for_each_enabled_repo<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(&Repo, &LocalRepo) -> Result<()>,
    {
        for repo_config in &self.config.repos {
            if !repo_config.enabled {
                continue;
            }

            match LocalRepo::new(self.config, repo_config.name.clone()) {
                Ok(local_repo) => {
                    if let Err(e) = callback(repo_config, &local_repo) {
                        eprintln!(
                            "{}",
                            format!("Warning: error processing repo '{}': {}", repo_config.name, e).yellow()
                        );
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        format!("Warning: skipping repo '{}': {}", repo_config.name, e).yellow()
                    );
                }
            }
        }
        Ok(())
    }

    /// Execute a callback for all repositories (enabled and disabled)
    pub fn for_all_repos<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(&Repo, &LocalRepo) -> Result<()>,
    {
        for repo_config in &self.config.repos {
            match LocalRepo::new(self.config, repo_config.name.clone()) {
                Ok(local_repo) => {
                    if let Err(e) = callback(repo_config, &local_repo) {
                        eprintln!(
                            "{}",
                            format!("Warning: error processing repo '{}': {}", repo_config.name, e).yellow()
                        );
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        format!("Warning: skipping repo '{}': {}", repo_config.name, e).yellow()
                    );
                }
            }
        }
        Ok(())
    }

    /// Get a list of all repositories with their local repo information
    pub fn list_repositories(&self) -> Result<Vec<(Repo, LocalRepo)>> {
        let mut repos = Vec::new();
        
        for repo_config in &self.config.repos {
            match LocalRepo::new(self.config, repo_config.name.clone()) {
                Ok(local_repo) => {
                    repos.push((repo_config.clone(), local_repo));
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        format!("Warning: skipping repo '{}': {}", repo_config.name, e).yellow()
                    );
                }
            }
        }
        
        Ok(repos)
    }

    /// Get detailed information about a specific repository
    pub fn get_repository_info(&self, name: &str) -> Result<LocalRepo> {
        LocalRepo::new(self.config, name.to_string())
    }

    /// List available subdirectories for a repository
    pub fn list_subdirectories(&self, name: &str) -> Result<Vec<String>> {
        let local_repo = LocalRepo::new(self.config, name.to_string())?;
        let mut subdirs = Vec::new();
        
        for dir in &local_repo.dotfile_dirs {
            if let Some(file_name) = dir.path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    subdirs.push(name_str.to_string());
                }
            }
        }
        
        Ok(subdirs)
    }

    /// Set active subdirectories for a repository
    pub fn set_subdirectories(&self, name: &str, subdirs: Vec<String>) -> Result<()> {
        // This would need mutable config access
        Err(anyhow::anyhow!("Use Config::set_active_subdirs directly for setting subdirectories"))
    }

    /// Get active dotfile directories from all enabled repositories
    // TODO: should this return a Vec<DotfileDir> instead?
    pub fn get_active_dotfile_dirs(&self) -> Result<Vec<std::path::PathBuf>> {
        let mut active_dirs = Vec::new();

        for repo_config in &self.config.repos {
            if !repo_config.enabled {
                continue;
            }

            match LocalRepo::new(self.config, repo_config.name.clone()) {
                Ok(local_repo) => {
                    for dir in &local_repo.dotfile_dirs {
                        if dir.is_active {
                            active_dirs.push(dir.path.clone());
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        format!("Warning: skipping repo '{}': {}", repo_config.name, e).yellow()
                    );
                }
            }
        }

        Ok(active_dirs)
    }
}

/// Extract a repository name from a git URL
/// This is a copy of the function from config.rs to avoid circular dependencies
fn extract_repo_name(repo: &str) -> String {
    let s = repo.trim_end_matches(".git");
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
