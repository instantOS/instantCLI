//! User management module
//!
//! This module provides functionality for managing system users.
//! It allows creating users, managing their shells, groups, and passwords.

mod handlers;
mod menu_items;
mod models;
mod ssh_keys;
mod system;
mod utils;

pub(super) use handlers::manage_users;
pub(super) use ssh_keys::{
    manage_ssh_keys, read_authorized_keys_for_helper, update_authorized_keys_for_helper,
};
pub(super) use system::get_current_user_info;

/// Shared with the installer's username question so both flows enforce the
/// same Unix name rules.
pub(crate) use utils::validate_username;
