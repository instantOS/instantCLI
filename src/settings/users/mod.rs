//! User management module
//!
//! This module provides functionality for managing system users through a TOML configuration.
//! It allows creating users, managing their shells, groups, and passwords.
//!
//! ## Module Structure
//!
//! - `models`: Data structures for user specifications and system user information
//! - `store`: Persistent storage for user configurations
//! - `menu_items`: UI menu items and FzfSelectable implementations
//! - `system`: System-level operations for querying and managing users/groups
//! - `utils`: Reusable utility functions for common operations
//! - `handlers`: Main UI handlers and business logic

mod handlers;
mod menu_items;
mod models;
mod store;
mod system;
mod utils;

// Re-export the public API
pub(super) use handlers::manage_users;

