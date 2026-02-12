//! User management module
//!
//! This module provides functionality for managing system users.
//! It allows creating users, managing their shells, groups, and passwords.

mod handlers;
mod menu_items;
mod models;
mod system;
mod utils;

pub(super) use handlers::manage_users;
