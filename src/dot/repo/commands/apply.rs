use crate::dot::apply_all;
use crate::dot::config::Config;
use crate::dot::db::Database;
use anyhow::Result;

/// Apply all repositories (helper function)
pub(super) fn apply_all_repos(config: &Config, db: &Database) -> Result<()> {
    apply_all(config, db)?;
    Ok(())
}
