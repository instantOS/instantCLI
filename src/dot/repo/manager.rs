use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use anyhow::Result;
use colored::Colorize;

/// RepositoryManager provides centralized iteration and management of repositories
/// following the existing borrowed references pattern
pub struct RepositoryManager<'a> {
    config: &'a Config,
    _db: &'a Database,
}

impl<'a> RepositoryManager<'a> {
    /// Create a new RepositoryManager with borrowed references
    pub fn new(config: &'a Config, db: &'a Database) -> Self {
        Self { config, _db: db }
    }

    /// Get detailed information about a specific repository
    pub fn get_repository_info(&self, name: &str) -> Result<LocalRepo> {
        LocalRepo::new(self.config, name.to_string())
    }

    /// Get active dotfile directories from all enabled repositories
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
