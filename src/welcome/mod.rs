//! Welcome application for instantOS first-time setup
//!
//! Provides a friendly introduction to instantOS with links to key resources
//! and the ability to configure autostart behavior.

pub mod commands;
mod ui;

pub use commands::{WelcomeCommands, handle_welcome_command};
