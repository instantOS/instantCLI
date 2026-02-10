use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::repo::DotfileRepositoryManager;
use anyhow::{Context, Result};
use colored::*;

/// Remove a repository
pub(super) fn remove_repository(
    config: &mut DotfileConfig,
    db: &Database,
    name: &str,
    remove_files: bool,
) -> Result<()> {
    // Find the repository
    let _repo_index = config
        .repos
        .iter()
        .position(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", name))?;

    if remove_files {
        // Remove the local files
        let repo_manager = DotfileRepositoryManager::new(config, db);
        if let Ok(local_repo) = repo_manager.get_repository_info(name) {
            let local_path = local_repo.local_path(config)?;
            if local_path.exists() {
                std::fs::remove_dir_all(&local_path).with_context(|| {
                    format!(
                        "Failed to remove repository directory: {}",
                        local_path.display()
                    )
                })?;
                println!("Removed repository files from: {}", local_path.display());
            }
        }
    }

    // Remove from config
    config.remove_repo(name, None)?;

    println!("{} repository '{}'", "Removed".green(), name);

    Ok(())
}
