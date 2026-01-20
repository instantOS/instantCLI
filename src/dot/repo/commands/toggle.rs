use crate::dot::config::Config;
use anyhow::Result;
use colored::*;

/// Enable a repository
pub(super) fn enable_repository(config: &mut Config, name: &str) -> Result<()> {
    config.enable_repo(name, None)?;
    println!("{} repository '{}'", "Enabled".green(), name);
    Ok(())
}

/// Disable a repository
pub(super) fn disable_repository(config: &mut Config, name: &str) -> Result<()> {
    config.disable_repo(name, None)?;
    println!("{} repository '{}'", "Disabled".yellow(), name);
    Ok(())
}

/// Set read-only status for a repository
pub fn set_read_only_status(config: &mut Config, name: &str, read_only: bool) -> Result<()> {
    for repo in &mut config.repos {
        if repo.name == name {
            repo.read_only = read_only;
            config.save(None)?;
            println!(
                "{} read-only status for repository '{}' to {}",
                "Set".green(),
                name,
                read_only
            );
            return Ok(());
        }
    }
    Err(anyhow::anyhow!("Repository '{}' not found", name))
}
