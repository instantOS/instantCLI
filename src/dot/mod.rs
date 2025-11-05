// Core modules
pub mod config;
pub mod db;
pub mod dotfile;
pub mod git;
pub mod localrepo;
pub mod meta;
pub mod path_serde;
pub mod repo;

// New organized modules
pub mod operations;
pub mod types;
pub mod utils;

#[cfg(test)]
mod path_tests;

// Re-exports for convenience
pub use crate::dot::dotfile::Dotfile;
pub use git::{diff_all, status_all, update_all};
pub use operations::{add_dotfile, apply_all, reset_modified};
pub use types::RepoName;

// Re-export utility functions
pub use utils::{get_all_dotfiles, resolve_dotfile_path};
