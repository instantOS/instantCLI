use std::path::PathBuf;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::dot::config::{ConfigManager, Repo, Config};
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use crate::dot::meta::read_meta;

/// Repository status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryStatus {
    pub name: String,
    pub enabled: bool,
    pub exists: bool,
    pub branch: Option<String>,
    pub clean: bool,
    pub has_changes: bool,
    pub active_subdirs: Vec<String>,
    pub available_subdirs: Vec<String>,
}

/// Detailed repository information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryInfo {
    pub name: String,
    pub url: String,
    pub branch: Option<String>,
    pub enabled: bool,
    pub local_path: PathBuf,
    pub exists: bool,
    pub active_subdirs: Vec<String>,
    pub available_subdirs: Vec<String>,
    pub description: Option<String>,
    pub last_updated: Option<String>,
}

/// Repository operation error with context
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Repository not found: {0}")]
    NotFound(String),

    #[error("Repository already exists: {0}")]
    AlreadyExists(String),

    #[error("Git operation failed: {0}")]
    GitOperation(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Filesystem error: {0}")]
    Filesystem(String),

    #[error("Database error: {0}")]
    Database(String),
}


/// Result type for repository operations
pub type RepositoryResult<T> = Result<T, RepositoryError>;

impl From<anyhow::Error> for RepositoryError {
    fn from(err: anyhow::Error) -> Self {
        RepositoryError::GitOperation(err.to_string())
    }
}

/// Unified repository service that handles all repository operations
pub struct RepositoryService {
    config_manager: ConfigManager,
    db: Database,
}

impl RepositoryService {
    /// Create a new repository service
    pub fn new(config_manager: ConfigManager, db: Database) -> Self {
        Self { config_manager, db }
    }

    /// Get a reference to the config manager
    pub fn config(&self) -> &Config {
        self.config_manager.config()
    }

    /// Get all configured repositories
    pub fn get_repositories(&self) -> Vec<&Repo> {
        self.config_manager.config().repos.iter().collect()
    }

    /// Get a specific repository by name
    pub fn get_repository(&self, name: &str) -> RepositoryResult<&Repo> {
        self.config_manager.config().repos.iter()
            .find(|repo| repo.name == name)
            .ok_or_else(|| RepositoryError::NotFound(name.to_string()))
    }

    /// Add a new repository
    pub fn add_repository(&mut self, url: &str, name: Option<&str>, branch: Option<&str>) -> RepositoryResult<()> {
        let repo_name = name.unwrap_or_else(|| {
            // Extract name from URL if not provided
            url.split('/')
                .last()
                .and_then(|s| s.strip_suffix(".git"))
                .unwrap_or("unknown")
        });

        // Check if repository already exists
        if self.get_repository(repo_name).is_ok() {
            return Err(RepositoryError::AlreadyExists(repo_name.to_string()));
        }

        // Clone the repository first
        self.clone_repository(url, repo_name, branch.unwrap_or("main"))?;

        // Add to configuration
        let repo_config = Repo {
            url: url.to_string(),
            name: repo_name.to_string(),
            branch: branch.map(|s| s.to_string()),
            active_subdirectories: vec!["dots".to_string()],
            enabled: true,
        };

        self.config_manager.add_repo(repo_config)
            .map_err(|e| RepositoryError::Configuration(e.to_string()))?;

        Ok(())
    }

    /// Remove a repository
    pub fn remove_repository(&mut self, name: &str) -> RepositoryResult<()> {
        // Check if repository exists
        self.get_repository(name)?;

        // Remove local files if they exist
        let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string())?;
        let local_path = local_repo.local_path(&self.config_manager.config())?;
        if local_path.exists() {
            std::fs::remove_dir_all(&local_path)
                .map_err(|e| RepositoryError::Filesystem(e.to_string()))?;
        }

        // Remove from configuration
        self.config_manager.remove_repo(name)
            .map_err(|e| RepositoryError::Configuration(e.to_string()))?;

        Ok(())
    }

    /// Enable a repository
    pub fn enable_repository(&mut self, name: &str) -> RepositoryResult<()> {
        let repo = self.get_repository(name)?;
        if repo.enabled {
            return Ok(()); // Already enabled
        }

        self.config_manager.with_config_mut(|config| {
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == name) {
                repo.enabled = true;
            }
            Ok(())
        }).map_err(|e| RepositoryError::Configuration(e.to_string()))?;

        Ok(())
    }

    /// Disable a repository
    pub fn disable_repository(&mut self, name: &str) -> RepositoryResult<()> {
        let repo = self.get_repository(name)?;
        if !repo.enabled {
            return Ok(()); // Already disabled
        }

        self.config_manager.with_config_mut(|config| {
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == name) {
                repo.enabled = false;
            }
            Ok(())
        }).map_err(|e| RepositoryError::Configuration(e.to_string()))?;

        Ok(())
    }

    /// Update a specific repository
    pub fn update_repository(&self, name: &str) -> RepositoryResult<()> {
        let repo = self.get_repository(name)?;
        if !repo.enabled {
            return Ok(());
        }

        let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string())?;
        let local_path = local_repo.local_path(&self.config_manager.config())?;
        if !local_path.exists() {
            return Err(RepositoryError::NotFound(name.to_string()));
        }

        local_repo.update(&self.config_manager.config(), false)
            .map_err(|e| RepositoryError::GitOperation(e.to_string()))?;

        Ok(())
    }

    /// Update all enabled repositories
    pub fn update_all_repositories(&self) -> RepositoryResult<()> {
        for repo in self.get_repositories() {
            if repo.enabled {
                if let Err(e) = self.update_repository(&repo.name) {
                    eprintln!("Failed to update repository {}: {}", repo.name, e);
                }
            }
        }
        Ok(())
    }

    /// Get repository status
    pub fn get_repository_status(&self, name: &str) -> RepositoryResult<RepositoryStatus> {
        let repo = self.get_repository(name)?;
        let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string());
        let exists = if let Ok(local_repo) = local_repo.as_ref() {
    if let Ok(local_path) = local_repo.local_path(&self.config_manager.config()) {
        local_path.exists()
    } else {
        false
    }
} else {
    false
};

        let (branch, clean, has_changes) = if exists {
            let local_repo = local_repo?;
            let _local_path = local_repo.local_path(&self.config_manager.config())?;
            let branch_result = local_repo.get_checked_out_branch(&self.config_manager.config());
            // For status and clean state, we'll need to implement these or use git commands directly
            // For now, we'll use placeholder values
            (
                branch_result.ok(),
                true, // placeholder
                false, // placeholder
            )
        } else {
            (None, false, false)
        };

        let available_subdirs = if exists {
            let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string())?;
            local_repo.dotfile_dirs.iter()
                .map(|d| d.path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string())
                .collect()
        } else {
            Vec::new()
        };

        Ok(RepositoryStatus {
            name: repo.name.clone(),
            enabled: repo.enabled,
            exists,
            branch,
            clean,
            has_changes,
            active_subdirs: repo.active_subdirectories.clone(),
            available_subdirs,
        })
    }

    /// Get detailed repository information
    pub fn get_repository_info(&self, name: &str) -> RepositoryResult<RepositoryInfo> {
        let repo = self.get_repository(name)?;
        let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string());
        let exists = if let Ok(local_repo) = local_repo.as_ref() {
    if let Ok(local_path) = local_repo.local_path(&self.config_manager.config()) {
        local_path.exists()
    } else {
        false
    }
} else {
    false
};

        let mut description = None;
        let mut last_updated = None;

        if let Ok(ref local_repo) = local_repo {
            let local_path = local_repo.local_path(&self.config_manager.config())?;
            if let Ok(meta) = read_meta(&local_path) {
                description = meta.description;
            }

            // Try to get last updated time from git
            if let Ok(output) = std::process::Command::new("git")
                .args(["log", "-1", "--format=%ci"])
                .current_dir(&local_path)
                .output()
            {
                if output.status.success() {
                    last_updated = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                }
            }
        }

        let available_subdirs = if exists {
            let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string())?;
            local_repo.dotfile_dirs.iter()
                .map(|d| d.path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string())
                .collect()
        } else {
            Vec::new()
        };

        Ok(RepositoryInfo {
            name: repo.name.clone(),
            url: repo.url.clone(),
            branch: repo.branch.clone(),
            enabled: repo.enabled,
            local_path: if let Ok(local_repo) = local_repo {
                local_repo.local_path(&self.config_manager.config()).unwrap_or_default()
            } else {
                PathBuf::default()
            },
            exists,
            active_subdirs: repo.active_subdirectories.clone(),
            available_subdirs,
            description,
            last_updated,
        })
    }

    /// List all repositories with their status
    pub fn list_repositories(&self) -> Vec<RepositoryStatus> {
        self.get_repositories().iter()
            .filter_map(|repo| {
                self.get_repository_status(&repo.name).ok()
            })
            .collect()
    }

    /// Set active subdirectories for a repository
    pub fn set_active_subdirectories(&mut self, name: &str, subdirs: Vec<String>) -> RepositoryResult<()> {
        // Check if repository exists
        self.get_repository(name)?;

        // Validate that subdirectories exist
        let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string());
        if let Ok(ref local_repo) = local_repo {
            let local_path = local_repo.local_path(&self.config_manager.config())?;
            if local_path.exists() {
                let available_subdirs: Vec<String> = local_repo.dotfile_dirs.iter()
                    .map(|d| d.path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string())
                    .collect();

                for subdir in &subdirs {
                    if !available_subdirs.contains(subdir) {
                        return Err(RepositoryError::Configuration(
                            format!("Subdirectory '{}' not found in repository '{}'. Available: {:?}",
                                    subdir, name, available_subdirs)
                        ));
                    }
                }
            }
        }

        self.config_manager.with_config_mut(|config| {
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == name) {
                repo.active_subdirectories = subdirs;
            }
            Ok(())
        }).map_err(|e| RepositoryError::Configuration(e.to_string()))?;

        Ok(())
    }

    /// Get available subdirectories for a repository
    pub fn get_available_subdirectories(&self, name: &str) -> RepositoryResult<Vec<String>> {
        let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string())?;
        let local_path = local_repo.local_path(&self.config_manager.config())?;
        if !local_path.exists() {
            return Ok(Vec::new());
        }

        let subdirs: Vec<String> = local_repo.dotfile_dirs.iter()
            .map(|d| d.path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string())
            .collect();

        Ok(subdirs)
    }

    /// Clone a repository to local filesystem
    fn clone_repository(&self, url: &str, name: &str, branch: &str) -> RepositoryResult<()> {
        let repos_dir = self.config_manager.config().repos_path();
        let local_path = repos_dir.join(name);

        // Create parent directory if it doesn't exist
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RepositoryError::Filesystem(e.to_string()))?;
        }

        // Clone the repository
        let output = std::process::Command::new("git")
            .args(["clone", "-b", branch, "--depth", "1", url, local_path.to_str().unwrap()])
            .output()
            .map_err(|e| RepositoryError::GitOperation(e.to_string()))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(RepositoryError::GitOperation(
                format!("Failed to clone repository: {}", error_msg)
            ));
        }

        Ok(())
    }

    /// Get all enabled repositories
    pub fn get_enabled_repositories(&self) -> Vec<&Repo> {
        self.get_repositories().into_iter()
            .filter(|repo| repo.enabled)
            .collect()
    }

    /// Get repository by name with local filesystem state
    pub fn get_local_repository(&self, name: &str) -> RepositoryResult<LocalRepo> {
        self.get_repository(name)?; // Ensure it exists in config

        let local_repo = LocalRepo::new(&self.config_manager.config(), name.to_string())?;
        let local_path = local_repo.local_path(&self.config_manager.config())?;
        if !local_path.exists() {
            return Err(RepositoryError::NotFound(name.to_string()));
        }

        Ok(local_repo)
    }

    /// Check if a repository exists and is enabled
    pub fn is_repository_active(&self, name: &str) -> bool {
        if let Ok(repo) = self.get_repository(name) {
            if repo.enabled {
                LocalRepo::new(&self.config_manager.config(), name.to_string())
                    .map(|r| r.local_path(&self.config_manager.config()).map(|p| p.exists()).unwrap_or(false))
                    .unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Get repository statistics
    pub fn get_repository_stats(&self) -> RepositoryStats {
        let repos = self.get_repositories();
        let enabled_count = repos.iter().filter(|r| r.enabled).count();
        let existing_count = repos.iter()
            .filter(|r| {
                LocalRepo::new(&self.config_manager.config(), r.name.clone())
                    .map(|repo| repo.local_path(&self.config_manager.config()).map(|p| p.exists()).unwrap_or(false))
                    .unwrap_or(false)
            })
            .count();

        RepositoryStats {
            total: repos.len(),
            enabled: enabled_count,
            existing: existing_count,
            disabled: repos.len() - enabled_count,
            missing: repos.len() - existing_count,
        }
    }
}

/// Repository statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryStats {
    pub total: usize,
    pub enabled: usize,
    pub existing: usize,
    pub disabled: usize,
    pub missing: usize,
}