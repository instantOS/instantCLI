use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::utils::get_all_dotfiles;
use crate::ui::prelude::*;
use anyhow::Result;
use std::path::PathBuf;

/// Apply all dotfiles from configured repositories
pub fn apply_all(config: &Config, db: &Database) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    for dotfile in all_dotfiles.values() {
        let was_missing = !dotfile.target_path.exists();
        dotfile.apply(db)?;

        if was_missing {
            let relative = dotfile
                .target_path
                .strip_prefix(&home)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to strip prefix from path {}: {}",
                        dotfile.target_path.display(),
                        e
                    )
                })?
                .to_string_lossy();
            emit(
                Level::Success,
                "dot.apply.created",
                &format!(
                    "{} Created new dotfile: ~/{relative}",
                    char::from(NerdFont::Check)
                ),
                None,
            );
        }
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;
    Ok(())
}

