pub mod config;
pub mod git;

pub use config::Repo;
pub use git::{add_repo, update_all, status_all};
