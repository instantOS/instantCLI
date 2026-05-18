pub mod add;
pub mod alternative;
pub mod apply;
pub mod decrypt;
pub mod delete;
pub mod encrypt;
pub mod git_commands;
pub mod key;
pub mod merge;
pub mod reset;

pub use add::add_dotfile;
pub use apply::apply_all;
pub use decrypt::decrypt_dotfile;
pub use delete::delete_dotfiles;
pub use encrypt::encrypt_dotfile;
pub use git_commands::{git_commit_all, git_pull_all, git_push_all, git_run_any};
pub use key::handle_key_command;
pub use reset::reset_modified;
