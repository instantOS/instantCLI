//! Setup module for instantCLI
//!
//! This module provides the `ins setup` command which handles integration setup
//! for various components like window managers.

mod commands;

pub(crate) use commands::generate_sway_config;
pub use commands::{SetupCommands, handle_setup_command};
