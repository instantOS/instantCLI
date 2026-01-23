//! Repository action handling for the dot menu

mod action_menu;
mod details;
mod handlers;
mod preview;

pub use handlers::handle_repo_actions;
pub use preview::build_repo_preview;
