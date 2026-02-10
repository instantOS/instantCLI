use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfilerepo::DotfileRepo;
use anyhow::Result;
use colored::Colorize;

/// DotfileRepositoryManager provides centralized iteration and management of dotfile repositories
/// following the existing borrowed references pattern
pub struct DotfileRepositoryManager<'a> {
    config: &'a DotfileConfig,
    _db: &'a Database,
}

impl<'a> DotfileRepositoryManager<'a> {
    /// Create a new DotfileRepositoryManager with borrowed references
    pub fn new(config: &'a DotfileConfig, db: &'a Database) -> Self {
        Self { config, _db: db }
    }

    /// Get detailed information about a specific repository
    pub fn get_repository_info(&self, name: &str) -> Result<DotfileRepo> {
        DotfileRepo::new(self.config, name.to_string())
    }

    /// Get active dotfile directories from all enabled repositories
    pub fn get_active_dotfile_dirs(&self) -> Result<Vec<std::path::PathBuf>> {
        let mut active_dirs = Vec::new();

        for repo_config in &self.config.repos {
            if !repo_config.enabled {
                continue;
            }

            match DotfileRepo::new(self.config, repo_config.name.clone()) {
                Ok(dotfile_repo) => {
                    for dir in dotfile_repo.active_dotfile_dirs() {
                        active_dirs.push(dir.path.clone());
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
