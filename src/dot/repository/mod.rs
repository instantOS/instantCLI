//! Unified repository management module
//!
//! This module provides a consolidated abstraction layer for all repository operations,
//! addressing the scattered logic and overlapping functionality found in the original codebase.

pub mod service;

pub use service::{
    RepositoryService, RepositoryStatus, RepositoryError,
};

use anyhow::Result;

use crate::dot::config::ConfigManager;
use crate::dot::db::Database;

/// Create a new repository service with the given configuration manager and database
pub fn create_repository_service(config_manager: ConfigManager, db: Database) -> RepositoryService {
    RepositoryService::new(config_manager, db)
}

/// Initialize a new repository in the current directory
pub fn init_repository(name: Option<&str>, description: Option<&str>) -> Result<()> {
    use crate::dot::meta::init_repo;

    let current_dir = std::env::current_dir()?;

    init_repo(&current_dir, name.as_deref(), true)?;

    let repo_name = name.unwrap_or_else(|| {
        // Use current directory name as default
        current_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
    });

    println!("Repository '{}' initialized successfully!", repo_name);
    if let Some(desc) = description {
        println!("Description: {}", desc);
    }
    println!("Dots directory: dots/");

    Ok(())
}

/// Validate repository configuration
pub fn validate_repository_config(config_manager: &ConfigManager) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let config = config_manager.config();

    for repo in &config.repos {
        // Check for duplicate names
        if config.repos.iter()
            .filter(|r| r.name == repo.name)
            .count() > 1
        {
            warnings.push(format!("Duplicate repository name: {}", repo.name));
        }

        // Check if URL is valid
        if !repo.url.starts_with("http://") && !repo.url.starts_with("https://") && !repo.url.starts_with("git@") {
            warnings.push(format!("Invalid URL format for repository {}: {}", repo.name, repo.url));
        }

        // Check if branch name is valid
        if repo.branch.as_ref().map_or(false, |b| b.is_empty()) {
            warnings.push(format!("Empty branch name for repository: {}", repo.name));
        }

        // Check if active subdirectories are specified
        if repo.active_subdirectories.is_empty() {
            warnings.push(format!("No active subdirectories for repository: {}", repo.name));
        }
    }

    Ok(warnings)
}

/// Get repository path for a given repository name
pub fn get_repository_path(config_manager: &ConfigManager, name: &str) -> Option<std::path::PathBuf> {
    let config = config_manager.config();
    if config.repos.iter().any(|r| r.name == name) {
        let repos_dir = config.repos_dir.as_path().to_path_buf();
        Some(repos_dir.join(name))
    } else {
        None
    }
}

/// Check if repository has uncommitted changes
pub fn has_uncommitted_changes(config_manager: &ConfigManager, name: &str) -> Result<bool> {
    let config = config_manager.config();
    let local_repo = crate::dot::localrepo::LocalRepo::new(config, name.to_string())?;
    let local_path = local_repo.local_path(config)?;

    if !local_path.exists() {
        return Ok(false);
    }

    // For now, return false as a placeholder
    // TODO: Implement proper git status checking
    Ok(false)
}

/// Get repository branch information
pub fn get_repository_branch(config_manager: &ConfigManager, name: &str) -> Result<Option<String>> {
    let config = config_manager.config();
    let local_repo = crate::dot::localrepo::LocalRepo::new(config, name.to_string())?;
    let local_path = local_repo.local_path(config)?;

    if !local_path.exists() {
        return Ok(None);
    }

    Ok(Some(local_repo.get_checked_out_branch(config)?))
}

/// Repository operations that work with multiple repositories
pub struct BulkRepositoryOperations {
    service: RepositoryService,
}

impl BulkRepositoryOperations {
    pub fn new(service: RepositoryService) -> Self {
        Self { service }
    }

    /// Update all repositories (sequential for now)
    pub fn update_all_sequential(&self) -> Vec<(String, RepositoryError)> {
        let mut errors = Vec::new();

        for repo in self.service.get_enabled_repositories() {
            if let Err(e) = self.service.update_repository(&repo.name) {
                errors.push((repo.name.clone(), e));
            }
        }

        errors
    }

    /// Get status of all repositories
    pub fn get_all_status(&self) -> Vec<RepositoryStatus> {
        self.service.list_repositories()
    }

    /// Clean up missing repositories from configuration
    pub fn cleanup_missing_repositories(&self) -> Result<usize> {
        // Note: This would require mutable access to the service's config
        // For now, we'll just return the count of repositories that would be removed
        let missing_count: usize = self.service.get_repositories()
            .iter()
            .filter(|repo| self.service.get_local_repository(&repo.name).is_err())
            .count();

        Ok(missing_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use crate::dot::config::{Config, ConfigManager, Repo};

    #[test]
    fn test_validate_repository_config() {
        let mut config = Config::default();

        // Add a valid repository
        config.repos.push(Repo {
            url: "https://github.com/test/repo.git".to_string(),
            name: "test-repo".to_string(),
            branch: Some("main".to_string()),
            active_subdirectories: vec!["dots".to_string()],
            enabled: true,
        });

        let config_manager = ConfigManager {
            config,
            custom_path: None
        };

        let warnings = validate_repository_config(&config_manager).unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_repository_stats() {
        let config = Config::default();
        let config_manager = ConfigManager {
            config,
            custom_path: None
        };
        let db = Database::new(PathBuf::from("test.db")).unwrap();
        let service = RepositoryService::new(config_manager, db);

        let stats = service.get_repository_stats();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.enabled, 0);
        assert_eq!(stats.existing, 0);
    }

    #[test]
    fn test_init_repository() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(&temp_dir).unwrap();

        // Initialize as git repository first (required by init_repo)
        git2::Repository::init(&temp_dir).unwrap();

        let result = init_repository(Some("test-repo"), Some("Test repository"));
        assert!(result.is_ok());

        // Check if instantdots.toml was created
        assert!(temp_dir.path().join("instantdots.toml").exists());

        std::env::set_current_dir(original_dir).unwrap();
    }
}