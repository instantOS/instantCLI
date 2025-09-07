pub mod config;
pub mod git;
pub mod localrepo;
pub mod meta;


pub use git::{add_repo, update_all, status_all};
