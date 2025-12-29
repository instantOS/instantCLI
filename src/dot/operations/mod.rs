pub mod add;
pub mod alternative;
pub mod apply;
pub mod git_commands;
pub mod merge;
pub mod reset;

pub use add::add_dotfile;
pub use apply::apply_all;
pub use git_commands::{git_commit_all, git_push_all, git_run_any};
pub use merge::merge_dotfile;
pub use reset::reset_modified;
