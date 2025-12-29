// Core modules
pub mod commands;
pub mod config;
pub mod db;
pub mod dotfile;
pub mod git;
pub mod localrepo;
pub mod meta;
pub mod override_config;
pub mod path_serde;
pub mod repo;

// New organized modules
pub mod operations;
pub mod types;
pub mod utils;

#[cfg(test)]
mod external_metadata_tests;
#[cfg(test)]
mod path_tests;

// Re-exports for convenience - these are used throughout the dot module
pub use crate::dot::dotfile::Dotfile;
pub use git::{diff_all, status_all, update_all};
pub use operations::{
    add_dotfile, apply_all, git_commit_all, git_push_all, git_run_any, merge_dotfile,
    reset_modified,
};
pub use types::RepoName;
pub use utils::{get_all_dotfiles, resolve_dotfile_path};
