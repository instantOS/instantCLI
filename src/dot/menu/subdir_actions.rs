//! Subdirectory action handling for the dot menu

mod action_menu;
mod defaults;
mod delete;
mod menu;
mod orphaned;

pub(crate) use menu::handle_manage_subdirs;
