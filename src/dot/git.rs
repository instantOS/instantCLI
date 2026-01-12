//! Git operations for dotfile management
//!
//! This module handles git-related operations for dotfile repositories,
//! including status display, diff functionality, and repository management.

pub mod diff;
pub mod repo_ops;
pub mod status;

// Re-export the main functions
pub use diff::diff_all;
pub use repo_ops::{add_repo, get_dotfile_dir_name, get_repo_name_for_dotfile, update_all};
pub use status::{show_single_file_status, show_status_summary};

/// Status function that handles both single file and summary display
pub fn status_all(
    cfg: &crate::dot::config::Config,
    path: Option<&str>,
    db: &crate::dot::db::Database,
    show_all: bool,
    show_sources: bool,
) -> anyhow::Result<()> {
    let all_dotfiles = crate::dot::get_all_dotfiles(cfg, db)?;

    if let Some(path_str) = path {
        // Show status for specific path
        show_single_file_status(path_str, &all_dotfiles, cfg, db, show_sources)?;
    } else {
        // Show summary and file list
        show_status_summary(&all_dotfiles, cfg, db, show_all, show_sources)?;
    }

    Ok(())
}
