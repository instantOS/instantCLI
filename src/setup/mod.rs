//! Setup module for instantCLI
//!
//! This module provides the `ins setup` command which handles integration setup
//! for various components like window managers.

mod commands;

pub use commands::{SetupCommands, handle_setup_command, setup_sway};
