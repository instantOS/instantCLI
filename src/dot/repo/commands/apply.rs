use crate::dot::apply_all;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use anyhow::Result;

/// Apply all repositories (helper function)
pub(super) fn apply_all_repos(
    config: &DotfileConfig,
    db: &Database,
    include_root: bool,
    root_only: bool,
) -> Result<()> {
    apply_all(config, db, include_root, root_only)?;
    Ok(())
}
