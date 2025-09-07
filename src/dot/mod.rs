pub mod config;
pub mod git;
pub mod localrepo;

pub use localrepo::LocalRepo;
pub use git::{add_repo, update_all, status_all};
